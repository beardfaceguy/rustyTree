//! `jwalk`-based parallel filesystem walker.
//!
//! Produces a fully aggregated, size-sorted [`Tree`]. Walks in parent-before-
//! children order so each entry's parent `NodeId` is already known by the
//! time we insert the child (see `path_to_id` map below).
//!
//! Errors from individual entries (e.g. `EACCES` on a subdir) are currently
//! swallowed: the offending entry is skipped and the walk continues. A real
//! error-collection channel will land with task #224 when the UI grows a
//! status bar that can show warnings.

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

    for entry_result in WalkDir::new(root).follow_links(false).skip_hidden(false) {
        if cancel.load(Ordering::Relaxed) {
            return Err(ScanError::Cancelled);
        }

        let entry = match entry_result {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        let md = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
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
        let (size_self, alloc_self) = match kind {
            NodeKind::Dir => (0, 0),
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
            // out under `--release`).
            let Some(p) = path.parent().and_then(|p| path_to_id.get(p).copied()) else {
                continue;
            };
            Some(p)
        };

        let id = tree.insert(parent, node);
        if matches!(kind, NodeKind::Dir) {
            path_to_id.insert(path.clone(), id);
        }

        entries = entries.saturating_add(1);
        bytes = bytes.saturating_add(size_self);

        progress(ScanProgress {
            entries,
            bytes,
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
}
