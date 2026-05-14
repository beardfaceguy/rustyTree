//! End-to-end smoke test: scan a fixture, then render and inspect the
//! buffer. This is the most basic guarantee the CLI provides — if
//! these fail, nothing else is worth running.

mod common;

use common::{assert_render_contains, make_fixture_tree, render_lines, scan_fixture};
use rustytree_core::view::Status;

#[test]
fn scanning_a_fixture_finishes_with_status_done_and_correct_totals() {
    let fixture = make_fixture_tree();
    let app = scan_fixture(fixture.path());

    let Status::Done {
        total_bytes,
        file_count,
        dir_count,
        ..
    } = app.status
    else {
        panic!("expected Status::Done, got {:?}", app.status);
    };

    // Fixture is 100 + 200 + 300 + 400 = 1000 logical bytes across
    // 4 files in 2 subdirectories (beta, gamma). The "dirs" count
    // is descendants only — not counting the root itself — which
    // matches what `Tree::aggregate` documents.
    assert_eq!(total_bytes, 1000, "fixture totals known to the test");
    assert_eq!(file_count, 4);
    assert_eq!(dir_count, 2);
}

#[test]
fn done_scan_renders_root_row_and_done_status_line() {
    let fixture = make_fixture_tree();
    let mut app = scan_fixture(fixture.path());

    let lines = render_lines(&mut app, 120, 20);

    // Header should mention rustyTree and the scanned path.
    assert!(
        lines.iter().any(|l| l.contains("rustyTree")),
        "header line missing 'rustyTree' marker: {lines:#?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains(&fixture.path().display().to_string())),
        "header line missing scanned path: {lines:#?}"
    );

    // Status line shows the "done in ..." marker.
    assert!(
        lines.iter().any(|l| l.contains("done in")),
        "status line missing 'done in' marker: {lines:#?}"
    );

    // Column header is rendered.
    let column_header = lines
        .iter()
        .find(|l| l.contains("Name") && l.contains("Size"))
        .expect("column header line missing");
    assert!(column_header.contains("Allocated"));
    assert!(column_header.contains("Modified"));
}

#[test]
fn fixture_root_is_pre_expanded_so_top_level_children_are_visible() {
    // The CLI auto-expands the root on scan completion (see
    // `RustyTreeApp::poll_scan`'s Done arm). This test pins that
    // behaviour: a fresh scan should show alpha.txt, beta, and
    // gamma immediately, without the user pressing a key.
    let fixture = make_fixture_tree();
    let mut app = scan_fixture(fixture.path());

    assert_render_contains(&mut app, "alpha.txt");
    assert_render_contains(&mut app, "beta");
    assert_render_contains(&mut app, "gamma");
}

#[test]
fn render_does_not_panic_on_zero_size_terminal() {
    // ratatui will hand us a 0×0 frame in degenerate cases (terminal
    // resized to nothing, or a sub-pane collapsed to 0). The render
    // path has size guards (see `render_body` and
    // `render_column_header` in ui.rs); this test just confirms it
    // doesn't blow up.
    let fixture = make_fixture_tree();
    let mut app = scan_fixture(fixture.path());

    // Width 1 / height 1 is the smallest a TestBackend will accept;
    // a 0-width TestBackend panics inside the buffer. 1×1 still
    // forces every draw helper to take its "area too small" branch.
    let lines = render_lines(&mut app, 1, 1);
    assert_eq!(lines.len(), 1);
}
