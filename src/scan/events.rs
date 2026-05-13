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

/// Failure modes a scan can report to the UI.
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("scan root {0:?} is not a directory")]
    NotADirectory(PathBuf),
    #[error("io error: {0}")]
    Io(String),
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
        self.cancel();
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

/// Spawn a worker thread that scans `root` and streams events through the
/// returned [`ScanHandle`].
pub fn start_scan(root: PathBuf) -> ScanHandle {
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let cancel_worker = cancel.clone();

    let join = std::thread::Builder::new()
        .name("rustytree-scan".into())
        .spawn(move || {
            let start = Instant::now();
            if !root.is_dir() {
                let _ = tx.send(ScanEvent::Error(ScanError::NotADirectory(root)));
                return;
            }
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
        .expect("spawn rustytree-scan worker thread");

    ScanHandle {
        cancel,
        rx,
        join: Some(join),
    }
}
