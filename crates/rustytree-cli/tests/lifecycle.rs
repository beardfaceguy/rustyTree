//! Higher-level lifecycle behaviours: the welcome screen, the
//! "no path configured" path, scan re-runs, and the cancel-with-no-
//! scan no-op.

mod common;

use common::{make_fixture_tree, render_lines, scan_fixture};
use rustytree_cli::app::{Command, RustyTreeApp};
use rustytree_core::view::Status;
use std::path::PathBuf;

#[test]
fn empty_path_does_not_panic_and_surfaces_a_user_error() {
    // `RustyTreeApp::start_scan` short-circuits on an empty path
    // (see app.rs::start_scan). We can't construct it via
    // `RustyTreeApp::new("")` directly because PathBuf::from("")
    // is non-empty as an OsStr; use the same path the real CLI
    // does — `app.path = PathBuf::new()`.
    let mut app = RustyTreeApp::new(PathBuf::new());
    app.dispatch(Command::StartScan);

    match &app.status {
        Status::Error(msg) => {
            assert!(
                msg.contains("no path"),
                "error message should explain the empty-path case: {msg:?}"
            );
        }
        other => panic!("expected Status::Error, got {other:?}"),
    }
}

#[test]
fn cancel_with_no_scan_running_is_a_no_op() {
    // The reviewer flagged this as a corner case worth pinning:
    // CancelScan when `app.scan` is None should leave the world
    // alone (no panic, no transition into Cancelled).
    let mut app = RustyTreeApp::new(PathBuf::from("/tmp"));
    let status_before = format!("{:?}", app.status);
    app.dispatch(Command::CancelScan);
    let status_after = format!("{:?}", app.status);
    assert_eq!(
        status_before, status_after,
        "CancelScan with no scan should not change status"
    );
}

#[test]
fn rescanning_keeps_search_string_and_rebuilds_visible_rows() {
    let fixture = make_fixture_tree();
    let mut app = scan_fixture(fixture.path());

    // Type a search query the same way the user would, via the
    // public command path. The query is the bit we want to outlive
    // a rescan; everything else (expanded set, selection, visible
    // rows) should be reset.
    app.dispatch(Command::EnterSearch);
    for c in "alpha".chars() {
        app.dispatch(Command::SearchPush(c));
    }
    app.dispatch(Command::SearchApply);
    assert_eq!(app.ui.search, "alpha");

    // Trigger a rescan and pump it to completion.
    app.dispatch(Command::StartScan);
    common::run_to_done(&mut app);

    assert!(
        matches!(app.status, Status::Done { .. }),
        "rescan should land in Done, got {:?}",
        app.status
    );
    // Search query should survive rescan (UiState::reset_for_new_scan
    // preserves it deliberately).
    assert_eq!(
        app.ui.search, "alpha",
        "search query should survive a rescan"
    );
    // The visible row list must be rebuilt by `poll_scan` after the
    // new Done event — empty visible_rows would mean we regressed
    // the bug fixed alongside this test suite (see
    // `poll_scan_populates_visible_rows_after_done_event`).
    assert!(
        !app.ui.visible_rows.is_empty(),
        "rescan must rebuild visible_rows; got an empty list"
    );
}

#[test]
fn poll_scan_populates_visible_rows_after_done_event() {
    // Regression test for the `poll_scan` early-return bug: when
    // the worker delivered `Done`, the `scan = None` line caused
    // the next iteration of poll_scan's inner loop to `return`
    // before ever reaching the `if rows_dirty { rebuild_visible_rows
    // }` block at the bottom. Result: `visible_rows.len() == 0`
    // after a clean scan, and the tree only appeared after the
    // user pressed a key that dirtied the rows again.
    //
    // The fix changes that `return` to a `break` so the rebuild
    // block runs unconditionally before poll_scan exits. This test
    // pins the contract: a fresh scan_fixture leaves visible_rows
    // populated with at least the root + its immediate children
    // (auto-expanded on Done).
    let fixture = make_fixture_tree();
    let app = scan_fixture(fixture.path());
    assert!(
        !app.ui.visible_rows.is_empty(),
        "visible_rows must be populated after Status::Done is reached \
         — regression check on poll_scan's rebuild path"
    );
    // Fixture has root + alpha.txt + beta + gamma at the top level
    // (4 rows visible after the auto-expand of root). Subdirs
    // beta/gamma are not yet expanded.
    assert!(
        app.ui.visible_rows.len() >= 4,
        "expected at least root + 3 children visible, got {} rows",
        app.ui.visible_rows.len()
    );
}

#[test]
fn welcome_screen_renders_when_no_tree_yet() {
    // Pre-scan render: no tree, no rows. The renderer should fall
    // through to its empty-state path without panicking.
    let mut app = RustyTreeApp::new(PathBuf::from("/nonexistent/will/not/scan"));
    let lines = render_lines(&mut app, 120, 20);
    assert!(
        lines.iter().any(|l| l.contains("rustyTree")),
        "welcome screen should still show the rustyTree banner: {lines:#?}"
    );
}
