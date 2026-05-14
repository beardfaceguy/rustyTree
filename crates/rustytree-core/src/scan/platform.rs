//! Cross-platform metadata extraction.
//!
//! The rest of the codebase calls [`extract`] and gets back a
//! [`PlatformMetadata`] regardless of OS. OS-specific behaviour is hidden
//! behind `cfg(unix)` / `cfg(windows)` private functions.
//!
//! Currently filled in:
//! - **Linux/macOS/BSD (unix):** allocated bytes via `blocks() * 512`,
//!   owner name via the `uzers` crate (cached), mtime via `metadata.modified()`.
//! - **Windows:** allocated bytes is `None` (a real `GetCompressedFileSize`
//!   lookup is deferred), owner is `None`, mtime via `metadata.modified()`.
//!
//! `allocated_bytes` is an `Option<u64>` precisely so that "we don't
//! know the on-disk size" reads as `None` rather than getting silently
//! aliased onto the logical size — the latter would make the Allocated
//! column meaningless on Windows.

use std::fs::Metadata;
use std::time::SystemTime;

/// Uniform metadata snapshot produced for every scanned entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlatformMetadata {
    /// Bytes the entry actually occupies on disk (after sparse-file holes,
    /// block alignment, etc.). `None` on platforms where this can't be
    /// derived from a `std::fs::Metadata` alone — specifically Windows,
    /// which needs a separate `GetCompressedFileSize`/`FILE_STANDARD_INFO`
    /// call to know the real allocation. The view layer renders `None`
    /// as [`crate::format::NA_PLACEHOLDER`] and the sort key sinks
    /// `None`-rows to the bottom in both directions.
    pub allocated_bytes: Option<u64>,
    /// Last modification time, if the OS reported one.
    pub mtime: Option<SystemTime>,
    /// Display name of the owning user, if resolvable. `None` on Windows
    /// (deferred) and on unix when the uid has no matching passwd entry.
    pub owner: Option<String>,
}

/// Extract platform metadata for an already-stat'd filesystem entry.
///
/// Takes an existing [`Metadata`] rather than a path so we don't hit the
/// filesystem twice; the caller already has metadata in hand from `jwalk`.
pub fn extract(md: &Metadata) -> PlatformMetadata {
    extract_impl(md)
}

#[cfg(unix)]
fn extract_impl(md: &Metadata) -> PlatformMetadata {
    use std::os::unix::fs::MetadataExt;
    use std::sync::OnceLock;
    use uzers::{Users, UsersCache};

    static CACHE: OnceLock<std::sync::Mutex<UsersCache>> = OnceLock::new();

    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(UsersCache::new()));
    let owner = match cache.lock() {
        Ok(c) => c
            .get_user_by_uid(md.uid())
            .map(|u| u.name().to_string_lossy().into_owned()),
        Err(_) => None,
    };

    PlatformMetadata {
        allocated_bytes: Some(md.blocks().saturating_mul(512)),
        mtime: md.modified().ok(),
        owner,
    }
}

// One impl for every non-unix platform (Windows today, plus anything
// exotic like wasi/redox that we haven't characterised). They all share
// the same "we can't derive on-disk size from std::fs::Metadata alone"
// constraint, so they all return `None` for `allocated_bytes`. Reporting
// `md.len()` here would silently masquerade as "allocated" in the
// Allocated column — exactly the divergence we don't want. A real
// `GetCompressedFileSize` / `FILE_STANDARD_INFO` lookup will land with
// the Windows on-disk-size task; until then `None` makes the
// unknown-ness explicit at every UI surface. The
// `allocated_bytes_is_none_on_non_unix` test below is gated on
// `cfg(not(unix))` so it covers Windows *and* any other non-unix host.
#[cfg(not(unix))]
fn extract_impl(md: &Metadata) -> PlatformMetadata {
    PlatformMetadata {
        allocated_bytes: None,
        mtime: md.modified().ok(),
        owner: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn extract_returns_modified_time_for_real_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("hello.txt");
        let mut f = std::fs::File::create(&path).expect("create");
        f.write_all(b"hello world").expect("write");
        f.sync_all().expect("sync");

        let md = std::fs::metadata(&path).expect("metadata");
        let pm = extract(&md);

        assert!(pm.mtime.is_some(), "mtime should be reported on this OS");
    }

    #[test]
    fn allocated_bytes_at_least_logical_on_unix() {
        // A non-sparse small file should report allocated_bytes >= logical.
        // On most filesystems a >0-byte file occupies at least one block.
        #[cfg(unix)]
        {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().join("small.bin");
            std::fs::write(&path, b"x").expect("write");
            let md = std::fs::metadata(&path).expect("metadata");
            let pm = extract(&md);
            let alloc = pm
                .allocated_bytes
                .expect("unix branch always returns Some(blocks*512)");
            assert!(
                alloc >= md.len(),
                "allocated_bytes ({alloc}) should be >= logical ({})",
                md.len()
            );
        }
    }

    #[test]
    #[cfg(not(unix))]
    fn allocated_bytes_is_none_on_non_unix() {
        // The non-unix branch (Windows today, anything else exotic
        // tomorrow) deliberately returns None until a real
        // GetCompressedFileSize / equivalent lookup lands. This guards
        // against someone "fixing" the Allocated column by dropping
        // md.len() back in.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("hello.bin");
        std::fs::write(&path, b"hello").expect("write");
        let md = std::fs::metadata(&path).expect("metadata");
        let pm = extract(&md);
        assert_eq!(pm.allocated_bytes, None);
    }

    #[test]
    #[cfg(unix)]
    fn owner_resolves_for_current_user() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("ownertest");
        std::fs::write(&path, b"").expect("write");
        let md = std::fs::metadata(&path).expect("metadata");
        let pm = extract(&md);
        assert!(
            pm.owner.is_some(),
            "expected owner name to resolve for files we just created"
        );
    }
}
