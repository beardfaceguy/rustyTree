//! `jwalk`-based parallel filesystem walker.
//!
//! Produces a fully aggregated, size-sorted [`Tree`]. Walks in parent-before-
//! children order so each entry's parent `NodeId` is already known by the
//! time we insert the child (see `path_to_id` map below).
//!
//! Per-entry I/O errors (`EACCES` on a subdir, `ENOENT` after a TOCTOU
//! race, etc.) don't fail the whole scan — the walker skips the offending
//! entry and keeps going, but increments a cumulative `errors` counter
//! that's reported through [`ScanProgress::errors`]. A future task can
//! upgrade this to a structured per-entry error channel; for now the
//! counter is enough to tell the user "your totals are partial".

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use jwalk::WalkDir;

use super::events::{ScanError, ScanProgress};
use super::platform;
use super::tree::{Node, NodeKind, Tree};

/// Walk `root` and return a fully-aggregated, size-sorted [`Tree`].
///
/// `cancel` is checked between entries; if it flips to `true` the walker
/// returns [`ScanError::Cancelled`].
///
/// `progress` is invoked periodically with cumulative entry/byte counts so
/// the UI can update a status line.
pub fn build_tree(
    root: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(ScanProgress),
) -> Result<Tree, ScanError> {
    let mut tree = Tree::new();
    let mut path_to_id: HashMap<PathBuf, super::tree::NodeId> = HashMap::new();

    let mut entries: u64 = 0;
    let mut bytes: u64 = 0;
    let mut errors: u64 = 0;

    for entry_result in WalkDir::new(root).follow_links(false).skip_hidden(false) {
        if cancel.load(Ordering::Relaxed) {
            return Err(ScanError::Cancelled);
        }

        let entry = match entry_result {
            Ok(e) => e,
            Err(_) => {
                errors = errors.saturating_add(1);
                continue;
            }
        };

        // jwalk yields a directory's `DirEntry` *successfully* even when
        // we lack permission to read its children (we can stat the dir
        // itself from above). It records the `EACCES` separately on
        // `DirEntry::read_children_error`. Without checking that field
        // we'd silently undercount: the dir would land in the tree with
        // zero children, totals would be wrong, and the user would have
        // no signal that anything was skipped.
        if entry.read_children_error.is_some() {
            errors = errors.saturating_add(1);
        }

        let path = entry.path();
        let md = match entry.metadata() {
            Ok(m) => m,
            Err(_) => {
                errors = errors.saturating_add(1);
                continue;
            }
        };

        let file_type = entry.file_type();
        let kind = if file_type.is_symlink() {
            NodeKind::Symlink
        } else if file_type.is_dir() {
            NodeKind::Dir
        } else {
            NodeKind::File
        };

        let pm = platform::extract(&md);
        // Directories don't contribute their own allocated bytes — the
        // children do — so they always report Some(0). Files/symlinks
        // forward whatever the platform gave us (Some on unix, None on
        // Windows / unsupported platforms).
        let (size_self, alloc_self): (u64, Option<u64>) = match kind {
            NodeKind::Dir => (0, Some(0)),
            NodeKind::File | NodeKind::Symlink => (md.len(), pm.allocated_bytes),
        };

        let name = if entry.depth() == 0 {
            path.display().to_string()
        } else {
            entry.file_name().to_string_lossy().into_owned()
        };

        let node = Node::new_leaf(name, kind, size_self, alloc_self, pm.mtime, pm.owner);

        let parent = if entry.depth() == 0 {
            None
        } else {
            // If the parent dir's metadata call failed earlier we may have
            // skipped the parent and never recorded its NodeId. Skip the
            // orphan child rather than passing `None` to `tree.insert` —
            // the latter would silently overwrite the root in release
            // builds (the `debug_assert!` in `Tree::insert` is compiled
            // out under `--release`). Count it as a skipped entry so the
            // user knows the totals are partial.
            let Some(p) = path.parent().and_then(|p| path_to_id.get(p).copied()) else {
                errors = errors.saturating_add(1);
                continue;
            };
            Some(p)
        };

        let id = tree.insert(parent, node)?;
        if matches!(kind, NodeKind::Dir) {
            path_to_id.insert(path.clone(), id);
        }

        entries = entries.saturating_add(1);
        bytes = bytes.saturating_add(size_self);

        progress(ScanProgress {
            entries,
            bytes,
            errors,
            current_path: path,
        });
    }

    tree.aggregate();
    tree.sort_children_by_size();
    Ok(tree)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    /// Build the same fixture used in `tree.rs` tests, but on disk:
    ///   <tmp>/root/a/a1.bin (100 bytes)
    ///   <tmp>/root/a/a2.bin (200 bytes)
    ///   <tmp>/root/b/b1.bin ( 50 bytes)
    ///   <tmp>/root/c.bin    (1000 bytes)
    fn make_fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("root");
        std::fs::create_dir(&root).unwrap();
        let a = root.join("a");
        std::fs::create_dir(&a).unwrap();
        std::fs::write(a.join("a1.bin"), vec![0u8; 100]).unwrap();
        std::fs::write(a.join("a2.bin"), vec![0u8; 200]).unwrap();
        let b = root.join("b");
        std::fs::create_dir(&b).unwrap();
        std::fs::write(b.join("b1.bin"), vec![0u8; 50]).unwrap();
        std::fs::write(root.join("c.bin"), vec![0u8; 1000]).unwrap();
        dir
    }

    #[test]
    fn build_tree_reports_correct_root_total() {
        let dir = make_fixture();
        let cancel = AtomicBool::new(false);
        let tree = build_tree(&dir.path().join("root"), &cancel, |_| {}).expect("scan");
        let root = tree.root().expect("root present");
        assert_eq!(tree.get(root).unwrap().size_total, 100 + 200 + 50 + 1000);
    }

    #[test]
    fn build_tree_sorts_children_by_size_descending() {
        let dir = make_fixture();
        let cancel = AtomicBool::new(false);
        let tree = build_tree(&dir.path().join("root"), &cancel, |_| {}).expect("scan");
        let root = tree.root().unwrap();
        let kids = &tree.get(root).unwrap().children;
        let totals: Vec<u64> = kids
            .iter()
            .map(|id| tree.get(*id).unwrap().size_total)
            .collect();
        // c.bin (1000), a/ (300), b/ (50)
        assert_eq!(totals, vec![1000, 300, 50]);
    }

    #[test]
    fn build_tree_counts_files_and_dirs() {
        let dir = make_fixture();
        let cancel = AtomicBool::new(false);
        let tree = build_tree(&dir.path().join("root"), &cancel, |_| {}).expect("scan");
        let root = tree.root().unwrap();
        let r = tree.get(root).unwrap();
        assert_eq!(r.file_count, 4);
        assert_eq!(r.dir_count, 2);
    }

    #[test]
    fn build_tree_respects_cancellation_flag() {
        let dir = make_fixture();
        let cancel = AtomicBool::new(true);
        let err = build_tree(&dir.path().join("root"), &cancel, |_| {}).unwrap_err();
        assert!(matches!(err, ScanError::Cancelled));
    }

    #[test]
    fn build_tree_calls_progress_callback() {
        let dir = make_fixture();
        let cancel = AtomicBool::new(false);
        let mut tick_count = 0u32;
        let _ = build_tree(&dir.path().join("root"), &cancel, |_| tick_count += 1).expect("scan");
        assert!(tick_count > 0, "progress should fire at least once");
    }

    #[test]
    fn build_tree_reports_zero_errors_on_clean_walk() {
        let dir = make_fixture();
        let cancel = AtomicBool::new(false);
        let mut last_errors: u64 = u64::MAX; // sentinel
        let _ = build_tree(&dir.path().join("root"), &cancel, |p| {
            last_errors = p.errors;
        })
        .expect("scan");
        assert_eq!(
            last_errors, 0,
            "clean fixture should not produce any skipped entries"
        );
    }

    #[cfg(unix)]
    #[test]
    fn build_tree_increments_errors_on_unreadable_subdir() {
        // Fixture: <tmp>/root/{readable/f.bin, locked/secret.bin}.
        // Strip read+execute permission from `locked` so jwalk's metadata
        // call on its children fails. The walker should skip those
        // entries, increment its `errors` counter, and still finish the
        // scan (totals from the readable side intact).
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("root");
        std::fs::create_dir(&root).unwrap();
        let readable = root.join("readable");
        std::fs::create_dir(&readable).unwrap();
        std::fs::write(readable.join("f.bin"), vec![0u8; 100]).unwrap();
        let locked = root.join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::write(locked.join("secret.bin"), vec![0u8; 50]).unwrap();
        // Strip all permissions on the locked dir AFTER populating it.
        let mut perms = std::fs::metadata(&locked).unwrap().permissions();
        perms.set_mode(0o000);
        std::fs::set_permissions(&locked, perms).unwrap();

        // Sanity-check that the permissions actually took effect; if
        // they didn't (e.g. tests running as root, where `mode 0o000`
        // is ignored) the rest of the test isn't meaningful, so skip
        // the assertion. Reading the locked dir would be the same
        // syscall that fails for the walker, so we use that as the
        // ground-truth gate.
        let permissions_actually_took_effect = std::fs::read_dir(&locked).is_err();

        let cancel = AtomicBool::new(false);
        let mut last_errors: u64 = 0;
        let res = build_tree(&root, &cancel, |p| last_errors = p.errors);

        // Restore permissions before any assertion so tempdir teardown
        // can recurse-delete the tree even if the test fails.
        let mut perms = std::fs::metadata(&locked).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&locked, perms).unwrap();

        let _tree = res.expect("scan completes despite unreadable subdir");
        if permissions_actually_took_effect {
            assert!(
                last_errors > 0,
                "expected at least one skipped entry under locked/, got {last_errors}"
            );
        }
    }
}
