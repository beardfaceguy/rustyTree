//! Scan event channel + cancellation handle.
//!
//! The UI spawns a scan with [`start_scan`], gets back a [`ScanHandle`], and
//! on every frame calls [`ScanHandle::try_recv`] to drain pending events.
//! Cancellation is cooperative: the worker thread checks an
//! `Arc<AtomicBool>` between filesystem entries and returns early when it
//! flips to `true`.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::tree::Tree;
use super::walker;

/// Per-tick progress payload. Sent at most every ~50ms while the walker
/// runs so the UI doesn't get flooded.
#[derive(Debug, Clone)]
pub struct ScanProgress {
    pub entries: u64,
    pub bytes: u64,
    pub current_path: PathBuf,
}

/// Events the UI receives from a running scan.
#[derive(Debug)]
pub enum ScanEvent {
    Progress(ScanProgress),
    Done { tree: Tree, elapsed: Duration },
    Cancelled,
    Error(ScanError),
}

/// Failure modes a scan can report to the UI. The first three are returned
/// synchronously from [`start_scan`]; `Cancelled` is delivered via the
/// channel as a [`ScanEvent::Cancelled`] (this variant exists so callers
/// who short-circuit straight to a `Result` can still represent the
/// cancelled state in their own error type if they want to).
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    /// The scan root path does not exist on the filesystem.
    #[error("scan root {0:?} does not exist")]
    NotFound(PathBuf),
    /// The scan root exists but is a file, symlink, or other non-directory.
    #[error("scan root {0:?} is not a directory")]
    NotADirectory(PathBuf),
    /// The OS refused to spawn the scan worker thread (e.g. resource
    /// exhaustion). The wrapped `io::Error` is what `Builder::spawn`
    /// returned.
    #[error("could not spawn scan worker thread: {0}")]
    SpawnFailed(std::io::Error),
    /// Scan was cancelled before it could finish.
    #[error("scan was cancelled")]
    Cancelled,
}

/// Owned handle to a running scan worker thread.
pub struct ScanHandle {
    cancel: Arc<AtomicBool>,
    rx: Receiver<ScanEvent>,
    join: Option<JoinHandle<()>>,
}

impl ScanHandle {
    /// Signal the worker to stop. The worker will check this flag between
    /// filesystem entries and emit [`ScanEvent::Cancelled`] before exiting.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// Non-blocking event drain.
    pub fn try_recv(&self) -> Result<ScanEvent, TryRecvError> {
        self.rx.try_recv()
    }

    /// True once the worker thread has exited.
    pub fn is_finished(&self) -> bool {
        match self.join.as_ref() {
            Some(j) => j.is_finished(),
            None => true,
        }
    }

    /// Block until the worker thread exits. Returns whatever the thread
    /// returned (which is currently always `()`).
    pub fn join(mut self) -> std::thread::Result<()> {
        match self.join.take() {
            Some(j) => j.join(),
            None => Ok(()),
        }
    }
}

impl Drop for ScanHandle {
    fn drop(&mut self) {
        // Set the cancel flag so the worker exits on its next per-entry
        // check. We *intentionally* do NOT join here: cancellation is
        // cooperative, so a worker stuck in a slow filesystem syscall
        // (hung NFS mount, sleeping disk) wouldn't observe the flag for
        // an arbitrary amount of time, and joining would freeze whichever
        // thread is dropping the handle (typically the UI thread on
        // rescan). The worker will still finish on its own and clean up
        // its end of the channel; nobody is reading by then.
        self.cancel();
        // `join` field is just dropped — the OS reaps the thread when it
        // exits. The `JoinHandle::drop` impl is documented as detaching.
        let _ = self.join.take();
    }
}

/// Spawn a worker thread that scans `root` and streams events through the
/// returned [`ScanHandle`].
///
/// Synchronous failures (`root` doesn't exist / isn't a directory / can't
/// spawn the worker thread) come back as `Err`. Once the worker is
/// running, all further outcomes (progress, completion, cancellation,
/// runtime I/O errors) flow through the [`ScanEvent`] channel.
pub fn start_scan(root: PathBuf) -> Result<ScanHandle, ScanError> {
    // Validate up front so a typo'd path produces an immediate, typed
    // error instead of being routed through the worker thread + channel.
    // Use `try_exists` so transient I/O errors during the existence check
    // (e.g. EACCES on the parent dir) don't masquerade as "doesn't exist".
    match root.try_exists() {
        Ok(true) => {}
        Ok(false) => return Err(ScanError::NotFound(root)),
        // Treat ambiguous existence the same as missing for now. A future
        // change can add a dedicated variant once we carry richer scan
        // diagnostics.
        Err(_) => return Err(ScanError::NotFound(root)),
    }
    if !root.is_dir() {
        return Err(ScanError::NotADirectory(root));
    }

    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let cancel_worker = cancel.clone();

    let join = std::thread::Builder::new()
        .name("rustytree-scan".into())
        .spawn(move || {
            let start = Instant::now();
            let tx_progress = tx.clone();
            let mut last_send = Instant::now();
            let progress = move |p: ScanProgress| {
                if last_send.elapsed() >= Duration::from_millis(50) {
                    let _ = tx_progress.send(ScanEvent::Progress(p));
                    last_send = Instant::now();
                }
            };
            match walker::build_tree(&root, &cancel_worker, progress) {
                Ok(tree) => {
                    let _ = tx.send(ScanEvent::Done {
                        tree,
                        elapsed: start.elapsed(),
                    });
                }
                Err(ScanError::Cancelled) => {
                    let _ = tx.send(ScanEvent::Cancelled);
                }
                Err(e) => {
                    let _ = tx.send(ScanEvent::Error(e));
                }
            }
        })
        .map_err(ScanError::SpawnFailed)?;

    Ok(ScanHandle {
        cancel,
        rx,
        join: Some(join),
    })
}
