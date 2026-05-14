//! End-to-end integration tests for the public `rustytree_core::scan` API.
//!
//! These exercise the actual worker thread + channel plumbing, not just the
//! synchronous `walker::build_tree` helper. They use `tempfile` fixtures so
//! they're hermetic and run in CI.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use rustytree_core::scan::{ScanEvent, start_scan};

/// Build:
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

/// Drain events from a handle until we hit a terminal state, with a timeout
/// so a buggy worker can't hang the test suite.
fn drain_until_terminal(handle: &rustytree_core::scan::ScanHandle) -> ScanEvent {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match handle.try_recv() {
            Ok(ev @ (ScanEvent::Done { .. } | ScanEvent::Cancelled | ScanEvent::Error(_))) => {
                return ev;
            }
            Ok(ScanEvent::Progress(_)) => {}
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                if Instant::now() > deadline {
                    panic!("scan did not finish within 10s");
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                panic!("worker disconnected without sending a terminal event");
            }
        }
    }
}

#[test]
fn full_scan_reports_done_with_correct_totals() {
    let dir = make_fixture();
    let handle = start_scan(dir.path().join("root")).expect("scan started");
    let ev = drain_until_terminal(&handle);
    let ScanEvent::Done { tree, elapsed: _ } = ev else {
        panic!("expected Done, got {ev:?}");
    };
    let root = tree.root().expect("root present");
    assert_eq!(tree.get(root).unwrap().size_total, 100 + 200 + 50 + 1000);
    assert_eq!(tree.get(root).unwrap().file_count, 4);
    assert_eq!(tree.get(root).unwrap().dir_count, 2);
}

#[test]
fn nonexistent_root_reports_not_found_via_channel() {
    let phantom = PathBuf::from("/this/path/does/not/exist/rustytree-test");
    let handle = start_scan(phantom).expect("spawn should succeed");
    match drain_until_terminal(&handle) {
        ScanEvent::Error(rustytree_core::scan::ScanError::NotFound(_)) => {}
        other => panic!("expected Error(NotFound), got {other:?}"),
    }
}

#[test]
fn file_root_reports_not_a_directory_via_channel() {
    let dir = tempfile::tempdir().expect("tempdir");
    let f = dir.path().join("not-a-directory.txt");
    std::fs::write(&f, b"this is a file, not a directory").unwrap();
    let handle = start_scan(f).expect("spawn should succeed");
    match drain_until_terminal(&handle) {
        ScanEvent::Error(rustytree_core::scan::ScanError::NotADirectory(_)) => {}
        other => panic!("expected Error(NotADirectory), got {other:?}"),
    }
}

#[test]
fn cancellation_short_circuits_a_running_scan() {
    // Build a slightly larger tree so cancellation has time to bite. We don't
    // need to be fancy: jwalk's per-entry overhead alone gives the cancel
    // flag a chance to flip mid-walk.
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("root");
    std::fs::create_dir(&root).unwrap();
    for i in 0..500 {
        let sub = root.join(format!("sub{i}"));
        std::fs::create_dir(&sub).unwrap();
        for j in 0..10 {
            std::fs::write(sub.join(format!("f{j}.bin")), vec![0u8; 32]).unwrap();
        }
    }

    let handle = start_scan(root).expect("scan started");
    handle.cancel();
    let ev = drain_until_terminal(&handle);
    // Either the worker had already finished by the time we cancelled (small
    // tree, fast disk) or it observed the flag and emitted Cancelled.
    match ev {
        ScanEvent::Cancelled | ScanEvent::Done { .. } => {}
        other => panic!("expected Cancelled or Done, got {other:?}"),
    }
}

#[test]
fn drop_does_not_block_the_caller() {
    // Spawn a scan on a fairly chunky tree, drop the handle on a worker
    // thread, and assert the drop completes inside a generous deadline.
    // If anyone reintroduces a `join()` in `Drop`, the worker will spend
    // seconds walking thousands of entries while the dropping thread is
    // stuck — this test catches that without a flaky wall-clock budget
    // (we check via channel signalling rather than `Instant::elapsed`).
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("root");
    std::fs::create_dir(&root).unwrap();
    for i in 0..200 {
        let sub = root.join(format!("sub{i}"));
        std::fs::create_dir(&sub).unwrap();
        for j in 0..20 {
            std::fs::write(sub.join(format!("f{j}.bin")), vec![0u8; 64]).unwrap();
        }
    }

    let handle = start_scan(root).expect("scan started");
    let (tx_done, rx_done) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        drop(handle);
        let _ = tx_done.send(());
    });
    // 2s is large enough to absorb worst-case scheduler jitter on Windows
    // CI runners but small enough to fail fast if Drop joins on a
    // ~4000-entry walk (which would take seconds even on a fast SSD).
    match rx_done.recv_timeout(Duration::from_secs(2)) {
        Ok(()) => {}
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            panic!("ScanHandle::drop did not return within 2s — Drop probably joined");
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            panic!("dropping thread panicked");
        }
    }
}
