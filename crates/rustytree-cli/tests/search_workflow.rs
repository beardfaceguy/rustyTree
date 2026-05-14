//! Cover the modal search workflow: enter Search mode with `/`, type
//! a query, hit Enter to apply, observe filtered rows, then clear or
//! abort to restore the unfiltered view.

mod common;

use common::{make_fixture_tree, press, render_lines, scan_fixture};
use crossterm::event::KeyCode;
use rustytree_cli::app::Mode;

#[test]
fn slash_enters_search_mode_and_esc_aborts_back_to_normal() {
    let fixture = make_fixture_tree();
    let mut app = scan_fixture(fixture.path());
    assert_eq!(app.mode, Mode::Normal);

    press(&mut app, KeyCode::Char('/'));
    assert_eq!(app.mode, Mode::Search);

    press(&mut app, KeyCode::Esc);
    assert_eq!(app.mode, Mode::Normal);
    assert!(
        app.ui.search.is_empty(),
        "Esc should drop the in-progress query"
    );
}

#[test]
fn typing_in_search_mode_pushes_into_the_query_string() {
    let fixture = make_fixture_tree();
    let mut app = scan_fixture(fixture.path());

    press(&mut app, KeyCode::Char('/'));
    for c in "alpha".chars() {
        press(&mut app, KeyCode::Char(c));
    }
    assert_eq!(app.ui.search, "alpha");
    assert_eq!(app.mode, Mode::Search);

    // Backspace removes one char.
    press(&mut app, KeyCode::Backspace);
    assert_eq!(app.ui.search, "alph");
}

#[test]
fn applying_a_search_filters_visible_rows_to_matching_subtrees() {
    let fixture = make_fixture_tree();
    let mut app = scan_fixture(fixture.path());

    // Type "alpha" then Enter.
    press(&mut app, KeyCode::Char('/'));
    for c in "alpha".chars() {
        press(&mut app, KeyCode::Char(c));
    }
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.mode, Mode::Normal);

    // alpha.txt is the only matching node; only its ancestor chain
    // (the root) plus alpha.txt itself should remain visible.
    let names: Vec<String> = app
        .ui
        .visible_rows
        .iter()
        .filter_map(|r| app.tree.as_ref()?.get(r.id))
        .map(|n| n.name().to_string())
        .collect();
    assert!(
        names.iter().any(|n| n == "alpha.txt"),
        "alpha.txt must remain visible: {names:?}"
    );
    // beta and gamma should no longer be visible — they don't
    // contain anything matching "alpha".
    assert!(
        !names.iter().any(|n| n == "beta"),
        "beta should be filtered out: {names:?}"
    );
    assert!(
        !names.iter().any(|n| n == "gamma"),
        "gamma should be filtered out: {names:?}"
    );
}

#[test]
fn clearing_search_restores_the_full_view() {
    let fixture = make_fixture_tree();
    let mut app = scan_fixture(fixture.path());

    // Apply a filter that knocks out beta and gamma.
    press(&mut app, KeyCode::Char('/'));
    for c in "alpha".chars() {
        press(&mut app, KeyCode::Char(c));
    }
    press(&mut app, KeyCode::Enter);
    assert!(app.ui.visible_rows.len() < 4, "filter should narrow rows");

    // `c` in normal mode is bound to SearchClear (see key_to_command).
    press(&mut app, KeyCode::Char('c'));
    assert_eq!(app.ui.search, "");

    // Once the search is cleared, beta and gamma come back.
    let rendered = render_lines(&mut app, 120, 30).join("\n");
    assert!(
        rendered.contains("beta"),
        "beta should reappear:\n{rendered}"
    );
    assert!(
        rendered.contains("gamma"),
        "gamma should reappear:\n{rendered}"
    );
}

#[test]
fn search_match_for_a_nested_file_brings_along_its_ancestors() {
    let fixture = make_fixture_tree();
    let mut app = scan_fixture(fixture.path());

    // "b1.txt" lives under beta/. Searching for it must keep beta
    // visible too (so the user can see the path), even though beta
    // wasn't manually expanded.
    press(&mut app, KeyCode::Char('/'));
    for c in "b1".chars() {
        press(&mut app, KeyCode::Char(c));
    }
    press(&mut app, KeyCode::Enter);

    let names: Vec<String> = app
        .ui
        .visible_rows
        .iter()
        .filter_map(|r| app.tree.as_ref()?.get(r.id))
        .map(|n| n.name().to_string())
        .collect();
    assert!(names.iter().any(|n| n == "b1.txt"), "match: {names:?}");
    assert!(names.iter().any(|n| n == "beta"), "ancestor: {names:?}");
    assert!(
        !names.iter().any(|n| n == "gamma"),
        "non-ancestor of match should be filtered: {names:?}"
    );
}

#[test]
fn rendered_search_bar_appears_only_in_search_mode() {
    let fixture = make_fixture_tree();
    let mut app = scan_fixture(fixture.path());

    let normal = render_lines(&mut app, 120, 20).join("\n");
    assert!(
        !normal.contains("Search:"),
        "Search bar should be hidden in normal mode:\n{normal}"
    );

    press(&mut app, KeyCode::Char('/'));
    for c in "alp".chars() {
        press(&mut app, KeyCode::Char(c));
    }
    let searching = render_lines(&mut app, 120, 20).join("\n");
    assert!(
        searching.contains("alp"),
        "in-progress query 'alp' should appear in render:\n{searching}"
    );
}
