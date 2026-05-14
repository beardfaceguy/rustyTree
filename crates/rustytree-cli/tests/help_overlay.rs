//! `?` toggles the help overlay. Verify both the state flag and that
//! the overlay actually appears in the rendered buffer.

mod common;

use common::{make_fixture_tree, press, render_lines, scan_fixture};
use crossterm::event::KeyCode;

#[test]
fn question_mark_toggles_help_open_state() {
    let fixture = make_fixture_tree();
    let mut app = scan_fixture(fixture.path());
    assert!(!app.help_open, "help starts closed");

    press(&mut app, KeyCode::Char('?'));
    assert!(app.help_open, "first ? opens help");

    press(&mut app, KeyCode::Char('?'));
    assert!(!app.help_open, "second ? closes help");
}

#[test]
fn open_help_overlay_renders_some_help_text() {
    let fixture = make_fixture_tree();
    let mut app = scan_fixture(fixture.path());

    press(&mut app, KeyCode::Char('?'));
    let lines = render_lines(&mut app, 120, 30).join("\n");

    // Don't pin the exact help wording — that's a UX nit unrelated
    // to the toggle's correctness. Just check that the overlay
    // brought *some* recognisable content forward, e.g. one of the
    // key-binding columns.
    let has_help_marker = lines.contains("Help") || lines.contains("help") || lines.contains("Key");
    assert!(
        has_help_marker,
        "help overlay should show some recognisable header; render:\n{lines}"
    );
}

#[test]
fn closed_help_overlay_does_not_obscure_the_tree_view() {
    let fixture = make_fixture_tree();
    let mut app = scan_fixture(fixture.path());

    // First open and immediately close the overlay.
    press(&mut app, KeyCode::Char('?'));
    press(&mut app, KeyCode::Char('?'));
    assert!(!app.help_open);

    // Tree-view rows should still be visible; alpha.txt was
    // visible pre-overlay, so it should be visible post-toggle.
    let lines = render_lines(&mut app, 120, 30).join("\n");
    assert!(
        lines.contains("alpha.txt"),
        "tree should resume showing alpha.txt after help is closed:\n{lines}"
    );
}
