//! Verify that the digit sort hotkeys (1..=7) correctly switch
//! `state.sort_key` and that pressing the same key twice flips the
//! direction. Behaviour is implemented in
//! `RustyTreeApp::dispatch(Command::SetSort)` and exercised here
//! through the real key path.

mod common;

use common::{make_fixture_tree, press, scan_fixture};
use crossterm::event::KeyCode;
use rustytree_core::view::{SortDir, SortKey};

#[test]
fn pressing_digit_keys_switches_sort_key() {
    let fixture = make_fixture_tree();
    let mut app = scan_fixture(fixture.path());

    let cases = [
        (KeyCode::Char('1'), SortKey::Name),
        (KeyCode::Char('2'), SortKey::Size),
        (KeyCode::Char('3'), SortKey::Allocated),
        (KeyCode::Char('4'), SortKey::FileCount),
        (KeyCode::Char('5'), SortKey::DirCount),
        (KeyCode::Char('6'), SortKey::Mtime),
        (KeyCode::Char('7'), SortKey::Owner),
    ];
    for (code, expected) in cases {
        press(&mut app, code);
        assert_eq!(
            app.ui.sort_key, expected,
            "pressing {code:?} should set sort key to {expected:?}"
        );
    }
}

#[test]
fn pressing_the_same_sort_key_twice_flips_direction() {
    let fixture = make_fixture_tree();
    let mut app = scan_fixture(fixture.path());

    // Start fresh on Name (key 1). First press lands us on
    // SortKey::Name with the default direction for that key (Asc,
    // since names are most useful alphabetical).
    press(&mut app, KeyCode::Char('1'));
    let dir_after_first = app.ui.sort_dir;
    assert_eq!(app.ui.sort_key, SortKey::Name);

    press(&mut app, KeyCode::Char('1'));
    assert_eq!(app.ui.sort_key, SortKey::Name);
    assert_ne!(
        app.ui.sort_dir, dir_after_first,
        "second press on the active sort key should flip direction"
    );

    press(&mut app, KeyCode::Char('1'));
    assert_eq!(
        app.ui.sort_dir, dir_after_first,
        "third press should flip back to original direction"
    );
}

#[test]
fn sort_by_name_orders_visible_children_alphabetically_asc() {
    let fixture = make_fixture_tree();
    let mut app = scan_fixture(fixture.path());

    press(&mut app, KeyCode::Char('1')); // SortKey::Name
    if app.ui.sort_dir != SortDir::Asc {
        press(&mut app, KeyCode::Char('1')); // flip to Asc
    }

    // Visible rows after the auto-expanded root: alpha.txt, beta,
    // gamma. With Name ascending, that's alphabetical.
    let names: Vec<String> = app
        .ui
        .visible_rows
        .iter()
        .skip(1) // skip the root row
        .filter_map(|r| app.tree.as_ref()?.get(r.id))
        .map(|n| n.name().to_string())
        .collect();

    assert_eq!(names, vec!["alpha.txt", "beta", "gamma"]);
}

#[test]
fn sort_by_size_desc_puts_the_largest_top_level_entry_first() {
    let fixture = make_fixture_tree();
    let mut app = scan_fixture(fixture.path());

    press(&mut app, KeyCode::Char('2')); // SortKey::Size
    if app.ui.sort_dir != SortDir::Desc {
        press(&mut app, KeyCode::Char('2'));
    }

    // gamma (400 + dir) > beta (500) > alpha (100).
    // Wait — beta has 200 + 300 = 500 bytes; gamma has 400.
    // Largest is beta. Sanity-check.
    let names: Vec<String> = app
        .ui
        .visible_rows
        .iter()
        .skip(1)
        .filter_map(|r| app.tree.as_ref()?.get(r.id))
        .map(|n| n.name().to_string())
        .collect();
    assert_eq!(
        names.first().map(String::as_str),
        Some("beta"),
        "Size-Desc should put beta (500B) at the top: got {names:?}"
    );
    assert_eq!(
        names.last().map(String::as_str),
        Some("alpha.txt"),
        "Size-Desc should put alpha.txt (100B) last: got {names:?}"
    );
}
