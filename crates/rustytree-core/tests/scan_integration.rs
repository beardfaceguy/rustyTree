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
fn nonexistent_root_returns_not_found_synchronously() {
    let phantom = PathBuf::from("/this/path/does/not/exist/rustytree-test");
    match start_scan(phantom) {
        Err(rustytree_core::scan::ScanError::NotFound(_)) => {}
        Err(other) => panic!("expected Err(NotFound), got Err({other:?})"),
        Ok(_) => panic!("expected Err(NotFound), got Ok(handle)"),
    }
}

#[test]
fn file_root_returns_not_a_directory_synchronously() {
    let dir = tempfile::tempdir().expect("tempdir");
    let f = dir.path().join("not-a-directory.txt");
    std::fs::write(&f, b"this is a file, not a directory").unwrap();
    match start_scan(f) {
        Err(rustytree_core::scan::ScanError::NotADirectory(_)) => {}
        Err(other) => panic!("expected Err(NotADirectory), got Err({other:?})"),
        Ok(_) => panic!("expected Err(NotADirectory), got Ok(handle)"),
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
    // Spawn a scan on a fairly chunky tree, drop the handle immediately,
    // and assert we return promptly. Joining on Drop would block until
    // the worker finished walking thousands of entries; detaching means
    // we return as soon as the cancel flag is flipped (microseconds).
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
    let start = Instant::now();
    drop(handle);
    let elapsed = start.elapsed();
    // 250ms is generous: dropping should be near-instant. If anyone adds a
    // join back into Drop and the worker is mid-walk, this fires.
    assert!(
        elapsed < Duration::from_millis(250),
        "ScanHandle::drop took {elapsed:?}, expected near-instant detach"
    );
}
