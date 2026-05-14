//! Cross-platform metadata extraction.
//!
//! The rest of the codebase calls [`extract`] and gets back a
//! [`PlatformMetadata`] regardless of OS. OS-specific behaviour is hidden
//! behind `cfg(unix)` / `cfg(windows)` private functions.
//!
//! Currently filled in:
//! - **Linux/macOS/BSD (unix):** allocated bytes via `blocks() * 512`,
//!   owner name via the `uzers` crate (cached), mtime via `metadata.modified()`.
//! - **Windows:** allocated bytes falls back to logical size (a real
//!   `GetCompressedFileSize` lookup is deferred), owner is `None`,
//!   mtime via `metadata.modified()`.

use std::fs::Metadata;
use std::time::SystemTime;

/// Uniform metadata snapshot produced for every scanned entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlatformMetadata {
    /// Bytes the entry actually occupies on disk (after sparse-file holes,
    /// block alignment, etc.). On Windows this currently falls back to the
    /// logical size.
    pub allocated_bytes: u64,
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
        allocated_bytes: md.blocks().saturating_mul(512),
        mtime: md.modified().ok(),
        owner,
    }
}

#[cfg(windows)]
fn extract_impl(md: &Metadata) -> PlatformMetadata {
    PlatformMetadata {
        allocated_bytes: md.len(),
        mtime: md.modified().ok(),
        owner: None,
    }
}

#[cfg(not(any(unix, windows)))]
fn extract_impl(md: &Metadata) -> PlatformMetadata {
    PlatformMetadata {
        allocated_bytes: md.len(),
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
            assert!(
                pm.allocated_bytes >= md.len(),
                "allocated_bytes ({}) should be >= logical ({})",
                pm.allocated_bytes,
                md.len()
            );
        }
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
