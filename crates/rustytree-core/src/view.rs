//! Front-end-agnostic view model: sort keys, search filter, the flattened
//! row list, and the expanded/selected set. Both `rustytree-gui` and
//! `rustytree-cli` build their renderers on top of this; the rules for
//! ordering, search-match auto-expansion, and chevron glyphs are decided
//! here so the two front-ends behave identically.
//!
//! Nothing in this module pulls in a rendering crate; everything is plain
//! data plus pure functions.

use std::collections::HashSet;
use std::time::Duration;

use crate::scan::{NodeId, ScanProgress, Tree};

/// Sort field for child rows under any given parent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SortKey {
    #[default]
    Size,
    Name,
    Allocated,
    FileCount,
    DirCount,
    Mtime,
    Owner,
}

/// Sort direction. Defaults to descending because the headline use case
/// ("biggest stuff first") wants size-desc on launch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    #[default]
    Desc,
}

/// One on-screen row: a node id plus its depth in the tree (0 = root).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowEntry {
    pub id: NodeId,
    pub depth: u16,
}

/// Identifies one displayable column. The label is what shows in headers;
/// `sort_key()` says which sort field clicking the header (or pressing the
/// matching number key in the CLI) should apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnKind {
    Name,
    Size,
    PercentOfRoot,
    Allocated,
    FileCount,
    DirCount,
    Mtime,
    Owner,
}

impl ColumnKind {
    pub const fn label(self) -> &'static str {
        match self {
            ColumnKind::Name => "Name",
            ColumnKind::Size => "Size",
            ColumnKind::PercentOfRoot => "%",
            ColumnKind::Allocated => "Allocated",
            ColumnKind::FileCount => "Files",
            ColumnKind::DirCount => "Dirs",
            ColumnKind::Mtime => "Modified",
            ColumnKind::Owner => "Owner",
        }
    }

    /// Sort key activated by clicking this column's header.
    pub const fn sort_key(self) -> Option<SortKey> {
        match self {
            ColumnKind::Name => Some(SortKey::Name),
            ColumnKind::Size | ColumnKind::PercentOfRoot => Some(SortKey::Size),
            ColumnKind::Allocated => Some(SortKey::Allocated),
            ColumnKind::FileCount => Some(SortKey::FileCount),
            ColumnKind::DirCount => Some(SortKey::DirCount),
            ColumnKind::Mtime => Some(SortKey::Mtime),
            ColumnKind::Owner => Some(SortKey::Owner),
        }
    }
}

/// Display order of columns shared by every front-end. Each front-end
/// independently decides how wide to draw each column.
pub const COLUMNS: &[ColumnKind] = &[
    ColumnKind::Name,
    ColumnKind::Size,
    ColumnKind::PercentOfRoot,
    ColumnKind::Allocated,
    ColumnKind::FileCount,
    ColumnKind::DirCount,
    ColumnKind::Mtime,
    ColumnKind::Owner,
];

/// Headline status of the most recent (or in-progress) scan.
#[derive(Debug, Default)]
pub enum Status {
    #[default]
    Idle,
    Scanning,
    Done {
        elapsed: Duration,
        total_bytes: u64,
        file_count: u64,
        dir_count: u64,
    },
    Cancelled,
    Error(String),
}

/// Mutable view state that survives across frames but is recomputed eagerly
/// when sort / search / expansion changes (see [`rebuild_visible_rows`]).
#[derive(Default)]
pub struct UiState {
    pub expanded: HashSet<NodeId>,
    pub selected: Option<NodeId>,
    pub sort_key: SortKey,
    pub sort_dir: SortDir,
    pub search: String,
    pub visible_rows: Vec<RowEntry>,
    /// Cumulative `entries` value from the most recent Progress event.
    pub last_progress: Option<ScanProgress>,
    /// `true` whenever `visible_rows` no longer reflects the current
    /// `expanded` / `sort_*` / `search` / `tree` state and must be rebuilt.
    pub rows_dirty: bool,
}

/// Pick the toggle glyph for a row. Pure ASCII so it always renders, even
/// in fonts that lack the geometric chevrons or folder emoji. Two-space
/// no-children variant keeps column alignment consistent.
pub fn chevron_glyph(has_children: bool, expanded: bool) -> &'static str {
    match (has_children, expanded) {
        (false, _) => "  ",
        (true, true) => "- ",
        (true, false) => "+ ",
    }
}

/// One-line status text for the bottom bar / header. Same wording for GUI
/// and CLI so the two front-ends feel like the same product.
pub fn status_line(status: &Status, last_progress: Option<&ScanProgress>) -> String {
    use crate::format;
    match status {
        Status::Idle => "ready".into(),
        Status::Scanning => match last_progress {
            Some(p) => {
                let mut s = format!(
                    "scanning... {} entries, {} so far ({})",
                    p.entries,
                    format::bytes(p.bytes),
                    p.current_path.display()
                );
                if p.errors > 0 {
                    s.push_str(&format!(" — {} skipped (i/o errors)", p.errors));
                }
                s
            }
            None => "scanning...".into(),
        },
        Status::Done {
            elapsed,
            total_bytes,
            file_count,
            dir_count,
        } => {
            let base = format!(
                "done in {} | {} | {} files, {} dirs",
                format::elapsed(*elapsed),
                format::bytes(*total_bytes),
                file_count,
                dir_count
            );
            // Surface skipped-entry count from the most recent progress
            // tick so the user can tell the totals are partial. The
            // walker keeps `errors` monotonic so the last-seen value is
            // the final count.
            match last_progress {
                Some(p) if p.errors > 0 => {
                    format!("{base} — {} skipped (i/o errors)", p.errors)
                }
                _ => base,
            }
        }
        Status::Cancelled => "cancelled".into(),
        Status::Error(e) => format!("error: {e}"),
    }
}

/// Re-flatten the tree into the visible-rows list according to the current
/// expansion / sort / search state. While a search is active, ancestors of
/// any matching nodes are treated as expanded *for this flatten only* —
/// `state.expanded` is **not** mutated, so clearing the search returns the
/// tree to whatever the user manually had open before they started typing.
pub fn rebuild_visible_rows(tree: &Tree, state: &mut UiState) {
    state.visible_rows.clear();
    let Some(root) = tree.root() else { return };

    let needle = state.search.trim().to_lowercase();
    let filter_active = !needle.is_empty();
    let subtree_has_match = if filter_active {
        compute_subtree_matches(tree, &needle)
    } else {
        HashSet::new()
    };

    let mut stack: Vec<(NodeId, u16)> = vec![(root, 0)];
    while let Some((id, depth)) = stack.pop() {
        if filter_active && !subtree_has_match.contains(&id) {
            continue;
        }
        state.visible_rows.push(RowEntry { id, depth });
        // While the filter is active, every ancestor of a match counts as
        // "expanded" so the user can see the matching subtree without us
        // mutating their persistent expansion set.
        let expanded_for_this_flatten =
            state.expanded.contains(&id) || (filter_active && subtree_has_match.contains(&id));
        if expanded_for_this_flatten && let Some(node) = tree.get(id) {
            let mut children = node.children.clone();
            sort_children(&mut children, tree, state.sort_key, state.sort_dir);
            for c in children.iter().rev() {
                stack.push((*c, depth + 1));
            }
        }
    }
}

/// Toggle a node's presence in the expanded set. Returns the new state
/// (`true` = now expanded).
pub fn toggle_expand(expanded: &mut HashSet<NodeId>, id: NodeId) -> bool {
    if expanded.remove(&id) {
        false
    } else {
        expanded.insert(id);
        true
    }
}

/// Sort a slice of child node ids in place by the given key/dir, looking
/// each node up in `tree`. Stable so equal-keyed siblings keep insertion
/// order (deterministic for tests).
pub fn sort_children(children: &mut [NodeId], tree: &Tree, key: SortKey, dir: SortDir) {
    children.sort_by(|a, b| {
        let na = match tree.get(*a) {
            Some(n) => n,
            None => return std::cmp::Ordering::Equal,
        };
        let nb = match tree.get(*b) {
            Some(n) => n,
            None => return std::cmp::Ordering::Equal,
        };
        let ord = match key {
            SortKey::Size => na.size_total.cmp(&nb.size_total),
            SortKey::Allocated => na.alloc_total.cmp(&nb.alloc_total),
            SortKey::Name => na.name.to_lowercase().cmp(&nb.name.to_lowercase()),
            SortKey::FileCount => na.file_count.cmp(&nb.file_count),
            SortKey::DirCount => na.dir_count.cmp(&nb.dir_count),
            // Mtime/Owner are optional; rows without a value should always
            // sort to the bottom regardless of direction. The naive
            // `Option::cmp` orders `None < Some(_)`, which means flipping
            // direction floats unknown rows to the top — rarely what users
            // want when sorting.
            SortKey::Mtime => return cmp_option_none_last(&na.mtime, &nb.mtime, dir),
            SortKey::Owner => return cmp_option_none_last(&na.owner, &nb.owner, dir),
        };
        match dir {
            SortDir::Asc => ord,
            SortDir::Desc => ord.reverse(),
        }
    });
}

/// Compare two `Option<T>` values such that `None` is always treated as
/// "greater" than any `Some(_)` — i.e. unknown rows fall to the bottom of
/// the sorted output in *both* `Asc` and `Desc`. Direction only flips
/// the order between `Some` values.
fn cmp_option_none_last<T: Ord>(a: &Option<T>, b: &Option<T>, dir: SortDir) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (Some(x), Some(y)) => {
            let ord = x.cmp(y);
            match dir {
                SortDir::Asc => ord,
                SortDir::Desc => ord.reverse(),
            }
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Compute the set of node ids whose subtree contains at least one node
/// whose lowercased name contains `needle`. Includes the matching nodes
/// themselves and all of their ancestors up to the root.
fn compute_subtree_matches(tree: &Tree, needle: &str) -> HashSet<NodeId> {
    let mut hits: HashSet<NodeId> = HashSet::new();
    for id in tree.iter_ids() {
        if let Some(n) = tree.get(id)
            && n.name.to_lowercase().contains(needle)
        {
            hits.insert(id);
        }
    }
    let mut closure = hits.clone();
    for id in hits {
        let mut cur = tree.get(id).and_then(|n| n.parent);
        while let Some(p) = cur {
            if !closure.insert(p) {
                break;
            }
            cur = tree.get(p).and_then(|n| n.parent);
        }
    }
    closure
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::{Node, NodeKind};

    fn dir(name: &str) -> Node {
        Node::new_leaf(name, NodeKind::Dir, 0, 0, None, None)
    }
    fn file(name: &str, size: u64) -> Node {
        Node::new_leaf(name, NodeKind::File, size, size, None, None)
    }

    fn sample() -> Tree {
        let mut t = Tree::new();
        let r = t.insert(None, dir("root")).unwrap();
        let a = t.insert(Some(r), dir("alpha")).unwrap();
        t.insert(Some(a), file("a1.bin", 100)).unwrap();
        t.insert(Some(a), file("a2.txt", 200)).unwrap();
        let b = t.insert(Some(r), dir("beta")).unwrap();
        t.insert(Some(b), file("b1.bin", 50)).unwrap();
        t.insert(Some(r), file("c.bin", 1000)).unwrap();
        t.aggregate();
        t
    }

    #[test]
    fn chevron_is_plus_when_collapsed_and_minus_when_expanded() {
        assert_eq!(chevron_glyph(true, false), "+ ");
        assert_eq!(chevron_glyph(true, true), "- ");
    }

    #[test]
    fn chevron_is_blank_when_no_children_regardless_of_expanded() {
        assert_eq!(chevron_glyph(false, false), "  ");
        assert_eq!(chevron_glyph(false, true), "  ");
    }

    #[test]
    fn flatten_collapsed_root_shows_only_root() {
        let tree = sample();
        let mut state = UiState::default();
        rebuild_visible_rows(&tree, &mut state);
        assert_eq!(state.visible_rows.len(), 1);
    }

    #[test]
    fn flatten_expanded_root_shows_root_plus_children() {
        let tree = sample();
        let mut state = UiState::default();
        state.expanded.insert(tree.root().unwrap());
        rebuild_visible_rows(&tree, &mut state);
        assert_eq!(state.visible_rows.len(), 4);
    }

    #[test]
    fn search_filters_to_matching_subtrees_and_auto_expands() {
        let tree = sample();
        let mut state = UiState {
            search: "b1".into(),
            ..Default::default()
        };
        rebuild_visible_rows(&tree, &mut state);
        let names: Vec<&str> = state
            .visible_rows
            .iter()
            .map(|r| tree.get(r.id).unwrap().name.as_str())
            .collect();
        assert!(names.contains(&"b1.bin"), "got {names:?}");
        assert!(names.contains(&"beta"), "got {names:?}");
        assert!(!names.contains(&"alpha"), "got {names:?}");
        assert!(!names.contains(&"c.bin"), "got {names:?}");
    }

    #[test]
    fn clearing_search_restores_prior_expansion() {
        // The user manually expanded only `root`. They run a search
        // for "b1" (which is buried under `beta`), then clear the
        // search. After clearing, `state.expanded` should be exactly
        // what it was before they started typing — `beta` should NOT
        // remain expanded.
        let tree = sample();
        let root = tree.root().unwrap();
        let mut state = UiState::default();
        state.expanded.insert(root);
        let baseline = state.expanded.clone();

        // Apply a search that, in the old code, auto-inserted matches
        // and ancestors into state.expanded.
        state.search = "b1".into();
        rebuild_visible_rows(&tree, &mut state);

        // Clear the search and re-flatten.
        state.search.clear();
        rebuild_visible_rows(&tree, &mut state);

        assert_eq!(
            state.expanded, baseline,
            "search should not persist into state.expanded"
        );
    }

    #[test]
    fn search_is_case_insensitive() {
        let tree = sample();
        let mut state = UiState {
            search: "ALPHA".into(),
            ..Default::default()
        };
        rebuild_visible_rows(&tree, &mut state);
        let names: Vec<&str> = state
            .visible_rows
            .iter()
            .map(|r| tree.get(r.id).unwrap().name.as_str())
            .collect();
        assert!(names.contains(&"alpha"));
    }

    #[test]
    fn sort_by_name_ascending_orders_alphabetically() {
        let tree = sample();
        let mut state = UiState {
            sort_key: SortKey::Name,
            sort_dir: SortDir::Asc,
            ..Default::default()
        };
        state.expanded.insert(tree.root().unwrap());
        rebuild_visible_rows(&tree, &mut state);
        let names: Vec<&str> = state
            .visible_rows
            .iter()
            .skip(1)
            .map(|r| tree.get(r.id).unwrap().name.as_str())
            .collect();
        assert_eq!(names, vec!["alpha", "beta", "c.bin"]);
    }

    #[test]
    fn sort_by_size_descending_is_default() {
        let tree = sample();
        let mut state = UiState::default();
        state.expanded.insert(tree.root().unwrap());
        rebuild_visible_rows(&tree, &mut state);
        let names: Vec<&str> = state
            .visible_rows
            .iter()
            .skip(1)
            .map(|r| tree.get(r.id).unwrap().name.as_str())
            .collect();
        assert_eq!(names, vec!["c.bin", "alpha", "beta"]);
    }

    #[test]
    fn toggle_expand_flips_state() {
        let tree = sample();
        let root = tree.root().unwrap();
        let mut set = HashSet::new();
        assert!(toggle_expand(&mut set, root)); // now expanded
        assert!(set.contains(&root));
        assert!(!toggle_expand(&mut set, root)); // now collapsed
        assert!(!set.contains(&root));
    }

    #[test]
    fn status_line_surfaces_skipped_entries_while_scanning() {
        use std::path::PathBuf;
        let progress = ScanProgress {
            entries: 100,
            bytes: 4096,
            errors: 7,
            current_path: PathBuf::from("/tmp/x"),
        };
        let s = status_line(&Status::Scanning, Some(&progress));
        assert!(s.contains("100 entries"), "got {s:?}");
        assert!(s.contains("7 skipped"), "got {s:?}");
    }

    #[test]
    fn status_line_surfaces_skipped_entries_in_done_state() {
        use std::path::PathBuf;
        use std::time::Duration;
        let progress = ScanProgress {
            entries: 100,
            bytes: 4096,
            errors: 3,
            current_path: PathBuf::from("/tmp/x"),
        };
        let done = Status::Done {
            elapsed: Duration::from_millis(420),
            total_bytes: 4096,
            file_count: 90,
            dir_count: 7,
        };
        let s = status_line(&done, Some(&progress));
        assert!(s.contains("3 skipped"), "got {s:?}");
    }

    #[test]
    fn status_line_omits_skipped_text_when_no_errors() {
        use std::path::PathBuf;
        let progress = ScanProgress {
            entries: 100,
            bytes: 4096,
            errors: 0,
            current_path: PathBuf::from("/tmp/x"),
        };
        let s = status_line(&Status::Scanning, Some(&progress));
        assert!(!s.contains("skipped"), "got {s:?}");
    }

    #[test]
    fn cmp_option_none_last_keeps_none_at_bottom_in_both_directions() {
        use std::cmp::Ordering;
        let a: Option<u8> = Some(1);
        let b: Option<u8> = Some(2);
        let n: Option<u8> = None;

        // Some-vs-Some honours direction.
        assert_eq!(cmp_option_none_last(&a, &b, SortDir::Asc), Ordering::Less);
        assert_eq!(
            cmp_option_none_last(&a, &b, SortDir::Desc),
            Ordering::Greater
        );

        // Some-vs-None puts Some first regardless of direction.
        assert_eq!(cmp_option_none_last(&a, &n, SortDir::Asc), Ordering::Less);
        assert_eq!(cmp_option_none_last(&a, &n, SortDir::Desc), Ordering::Less);
        assert_eq!(
            cmp_option_none_last(&n, &a, SortDir::Asc),
            Ordering::Greater
        );
        assert_eq!(
            cmp_option_none_last(&n, &a, SortDir::Desc),
            Ordering::Greater
        );

        // None-vs-None is Equal.
        assert_eq!(cmp_option_none_last(&n, &n, SortDir::Asc), Ordering::Equal);
        assert_eq!(cmp_option_none_last(&n, &n, SortDir::Desc), Ordering::Equal);
    }

    /// Build a tree where two of the three top-level children have an
    /// `mtime` and one is `None`. Sort by `Mtime` and confirm the `None`
    /// row lands at the end in both ascending and descending order.
    #[test]
    fn mtime_sort_keeps_unknown_rows_at_bottom_in_both_directions() {
        use crate::scan::Node;
        use std::time::{Duration, UNIX_EPOCH};

        let mut t = Tree::new();
        let r = t.insert(None, dir("root")).unwrap();
        // older
        t.insert(
            Some(r),
            Node::new_leaf(
                "older",
                NodeKind::File,
                10,
                10,
                Some(UNIX_EPOCH + Duration::from_secs(1_000_000)),
                None,
            ),
        )
        .unwrap();
        // newer
        t.insert(
            Some(r),
            Node::new_leaf(
                "newer",
                NodeKind::File,
                10,
                10,
                Some(UNIX_EPOCH + Duration::from_secs(2_000_000)),
                None,
            ),
        )
        .unwrap();
        // unknown mtime — was previously floated to the top under Asc and
        // bottom under Desc; should now stay at the bottom in both.
        t.insert(
            Some(r),
            Node::new_leaf("unknown", NodeKind::File, 10, 10, None, None),
        )
        .unwrap();
        t.aggregate();

        let mut state = UiState::default();
        state.expanded.insert(r);
        state.sort_key = SortKey::Mtime;

        // Asc
        state.sort_dir = SortDir::Asc;
        rebuild_visible_rows(&t, &mut state);
        let asc: Vec<&str> = state
            .visible_rows
            .iter()
            .skip(1)
            .map(|row| t.get(row.id).unwrap().name.as_str())
            .collect();
        assert_eq!(
            asc,
            vec!["older", "newer", "unknown"],
            "asc should sort known mtimes ascending and put None last"
        );

        // Desc
        state.sort_dir = SortDir::Desc;
        rebuild_visible_rows(&t, &mut state);
        let desc: Vec<&str> = state
            .visible_rows
            .iter()
            .skip(1)
            .map(|row| t.get(row.id).unwrap().name.as_str())
            .collect();
        assert_eq!(
            desc,
            vec!["newer", "older", "unknown"],
            "desc should sort known mtimes descending and STILL put None last"
        );
    }

    /// Same idea as the mtime test but for `Owner`. Lexicographic order on
    /// the `Some` branch, `None` at the bottom regardless of direction.
    #[test]
    fn owner_sort_keeps_unknown_rows_at_bottom_in_both_directions() {
        use crate::scan::Node;

        let mut t = Tree::new();
        let r = t.insert(None, dir("root")).unwrap();
        t.insert(
            Some(r),
            Node::new_leaf(
                "alice_file",
                NodeKind::File,
                10,
                10,
                None,
                Some("alice".into()),
            ),
        )
        .unwrap();
        t.insert(
            Some(r),
            Node::new_leaf("bob_file", NodeKind::File, 10, 10, None, Some("bob".into())),
        )
        .unwrap();
        t.insert(
            Some(r),
            Node::new_leaf("noowner_file", NodeKind::File, 10, 10, None, None),
        )
        .unwrap();
        t.aggregate();

        let mut state = UiState::default();
        state.expanded.insert(r);
        state.sort_key = SortKey::Owner;

        state.sort_dir = SortDir::Asc;
        rebuild_visible_rows(&t, &mut state);
        let asc: Vec<&str> = state
            .visible_rows
            .iter()
            .skip(1)
            .map(|row| t.get(row.id).unwrap().name.as_str())
            .collect();
        assert_eq!(asc, vec!["alice_file", "bob_file", "noowner_file"]);

        state.sort_dir = SortDir::Desc;
        rebuild_visible_rows(&t, &mut state);
        let desc: Vec<&str> = state
            .visible_rows
            .iter()
            .skip(1)
            .map(|row| t.get(row.id).unwrap().name.as_str())
            .collect();
        assert_eq!(desc, vec!["bob_file", "alice_file", "noowner_file"]);
    }

    #[test]
    fn column_labels_match_expected_set() {
        let labels: Vec<&str> = COLUMNS.iter().map(|c| c.label()).collect();
        assert_eq!(
            labels,
            vec![
                "Name",
                "Size",
                "%",
                "Allocated",
                "Files",
                "Dirs",
                "Modified",
                "Owner"
            ]
        );
    }
}
