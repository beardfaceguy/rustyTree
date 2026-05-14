//! Drive the app through the real keymap (`on_key`) and assert that
//! selection / expansion / scrolling behave the way the renderer
//! eventually paints. The point is to catch regressions where the
//! keymap, the dispatch logic, and the rendered output drift apart.

mod common;

use common::{key, make_fixture_tree, press, render_lines, scan_fixture};
use crossterm::event::KeyCode;
use rustytree_cli::app::Action;

fn selected_name(app: &rustytree_cli::app::RustyTreeApp) -> Option<&str> {
    let id = app.ui.selected?;
    let tree = app.tree.as_ref()?;
    tree.get(id).map(|n| n.name())
}

#[test]
fn down_arrow_advances_selection_one_row() {
    let fixture = make_fixture_tree();
    let mut app = scan_fixture(fixture.path());

    // After the auto-expand of root the selection sits on the root
    // itself. Pressing Down once should move to the first child.
    let before = selected_name(&app).map(str::to_owned);
    let action = press(&mut app, KeyCode::Down);
    assert_eq!(action, Action::Redraw, "Down is a bound key");
    let after = selected_name(&app).map(str::to_owned);
    assert_ne!(
        before, after,
        "Down should have changed selection; was {before:?}"
    );
}

#[test]
fn j_and_k_match_arrow_keys() {
    // Vim bindings should produce the same Action and the same
    // selection delta as the arrow keys. Easiest way to cover that
    // is to drive two parallel apps and compare.
    let fixture = make_fixture_tree();
    let mut app_arrows = scan_fixture(fixture.path());
    let mut app_vim = scan_fixture(fixture.path());

    press(&mut app_arrows, KeyCode::Down);
    press(&mut app_arrows, KeyCode::Down);
    press(&mut app_vim, KeyCode::Char('j'));
    press(&mut app_vim, KeyCode::Char('j'));

    assert_eq!(
        selected_name(&app_arrows).map(str::to_owned),
        selected_name(&app_vim).map(str::to_owned),
        "Down x2 and j x2 must land on the same row"
    );

    press(&mut app_arrows, KeyCode::Up);
    press(&mut app_vim, KeyCode::Char('k'));

    assert_eq!(
        selected_name(&app_arrows).map(str::to_owned),
        selected_name(&app_vim).map(str::to_owned),
        "Up x1 and k x1 must land on the same row"
    );
}

#[test]
fn unbound_letter_returns_action_ignore() {
    // The CLI's dirty-flag redraw scheme depends on `on_key` returning
    // Action::Ignore for unbound keys. Pin that contract for at
    // least one realistic key — a letter that isn't on the normal-
    // mode keymap.
    let fixture = make_fixture_tree();
    let mut app = scan_fixture(fixture.path());
    let action = app.on_key(key(KeyCode::Char('z'))); // 'z' isn't bound in normal mode
    assert_eq!(action, Action::Ignore);
}

#[test]
fn expanding_a_subdir_reveals_its_children_in_the_render() {
    let fixture = make_fixture_tree();
    let mut app = scan_fixture(fixture.path());

    // Move down until we're sitting on the `beta` directory. The
    // sort defaults to Size-Desc, so the tallest child comes first;
    // rather than depending on that, just walk down by name.
    let target = "beta";
    for _ in 0..10 {
        match selected_name(&app) {
            Some(n) if n == target => break,
            _ => {
                press(&mut app, KeyCode::Down);
            }
        }
    }
    assert_eq!(
        selected_name(&app),
        Some(target),
        "test fixture should contain a 'beta' row reachable from root"
    );

    // Before expanding, b1.txt / b2.txt should not be visible.
    let pre = render_lines(&mut app, 120, 30).join("\n");
    assert!(
        !pre.contains("b1.txt"),
        "b1.txt should be hidden before expanding beta:\n{pre}"
    );

    // Expand and re-render.
    press(&mut app, KeyCode::Right);
    let post = render_lines(&mut app, 120, 30).join("\n");
    assert!(
        post.contains("b1.txt"),
        "b1.txt should be visible after expanding beta:\n{post}"
    );
    assert!(
        post.contains("b2.txt"),
        "b2.txt should be visible after expanding beta:\n{post}"
    );
}

#[test]
fn collapsing_an_expanded_subdir_hides_its_children() {
    let fixture = make_fixture_tree();
    let mut app = scan_fixture(fixture.path());

    // Walk to beta and expand it.
    for _ in 0..10 {
        match selected_name(&app) {
            Some("beta") => break,
            _ => {
                press(&mut app, KeyCode::Down);
            }
        }
    }
    press(&mut app, KeyCode::Right);
    assert!(
        render_lines(&mut app, 120, 30)
            .join("\n")
            .contains("b1.txt"),
        "precondition: beta expanded"
    );

    // Now collapse and confirm the child rows are gone.
    press(&mut app, KeyCode::Left);
    let post = render_lines(&mut app, 120, 30).join("\n");
    assert!(
        !post.contains("b1.txt"),
        "b1.txt should be hidden after collapsing beta:\n{post}"
    );
}

#[test]
fn end_key_jumps_to_the_last_visible_row() {
    let fixture = make_fixture_tree();
    let mut app = scan_fixture(fixture.path());

    press(&mut app, KeyCode::End);
    let last_id = app
        .ui
        .visible_rows
        .last()
        .map(|r| r.id)
        .expect("at least one visible row");
    assert_eq!(
        app.ui.selected,
        Some(last_id),
        "End should select the last visible row"
    );

    press(&mut app, KeyCode::Home);
    let first_id = app
        .ui
        .visible_rows
        .first()
        .map(|r| r.id)
        .expect("at least one visible row");
    assert_eq!(
        app.ui.selected,
        Some(first_id),
        "Home should select the first visible row (the root)"
    );
}
