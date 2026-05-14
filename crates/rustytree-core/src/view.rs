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
    /// Memoised result of [`compute_subtree_matches`]: the lowercased
    /// needle the cache was built for, the tree node count at the time
    /// it was built, and the resulting set of matching ids (matches
    /// themselves plus all of their ancestors). [`rebuild_visible_rows`]
    /// reuses this when the user hasn't changed the search string AND
    /// the tree size hasn't grown — typical idle / sort-toggle /
    /// expand-toggle cases. The tree size is part of the key because
    /// the arena is append-only during a scan: as new nodes arrive,
    /// the cache is naturally invalidated. Across scans the apps
    /// explicitly call [`UiState::invalidate_search_cache`] when they
    /// replace the [`Tree`].
    search_cache: Option<SearchCache>,
}

/// Internal memo for `rebuild_visible_rows`. See [`UiState::search_cache`].
struct SearchCache {
    needle: String,
    tree_len: usize,
    matches: HashSet<NodeId>,
}

impl UiState {
    /// Drop the search-match memo. Callers must invoke this when the
    /// underlying [`Tree`] is replaced (e.g. starting a new scan); the
    /// cache otherwise keys on `tree.len()` and would silently serve
    /// stale `NodeId`s if the new tree happened to have the same
    /// length as the previous one.
    pub fn invalidate_search_cache(&mut self) {
        self.search_cache = None;
    }

    /// Reset the view state for a fresh scan, preserving only the
    /// fields the user explicitly cares about across scans: the
    /// in-progress search string and the chosen sort column /
    /// direction. Everything else (expanded nodes, selection, the
    /// flattened row list, the search-match cache, the last-progress
    /// snapshot) goes back to defaults.
    ///
    /// Both front-ends call this when they swap in a new [`Tree`];
    /// keeping the logic on `UiState` itself ensures the GUI and CLI
    /// can't drift on what "starting a new scan" means.
    pub fn reset_for_new_scan(&mut self) {
        let preserved_search = std::mem::take(&mut self.search);
        let preserved_sort_key = self.sort_key;
        let preserved_sort_dir = self.sort_dir;
        *self = UiState::default();
        self.search = preserved_search;
        self.sort_key = preserved_sort_key;
        self.sort_dir = preserved_sort_dir;
    }
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
                    // "errors" not "skipped" — see ScanProgress::errors:
                    // one unreadable directory hides many entries but
                    // contributes a single error event.
                    s.push_str(&format!(" — {} i/o errors", p.errors));
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
            // Surface the I/O-error event count from the most recent
            // progress tick so the user can tell the totals are partial.
            // The walker keeps `errors` monotonic so the last-seen value
            // is the final count. Note this is "error events", not
            // "missing entries" — see ScanProgress::errors.
            match last_progress {
                Some(p) if p.errors > 0 => {
                    format!("{base} — {} i/o errors", p.errors)
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
    // Reuse the cached match-set when the needle and the tree size are
    // both unchanged from the previous build. A clean cache miss
    // (different needle, or the arena grew because a Progress event
    // landed more nodes) recomputes and stores the new memo. When the
    // filter is inactive we drop the cache so we don't pin the
    // potentially large `HashSet` for no reason.
    if !filter_active {
        state.search_cache = None;
    } else {
        let tree_len = tree.len();
        let needs_recompute = match &state.search_cache {
            Some(c) => c.needle != needle || c.tree_len != tree_len,
            None => true,
        };
        if needs_recompute {
            state.search_cache = Some(SearchCache {
                needle: needle.clone(),
                tree_len,
                matches: compute_subtree_matches(tree, &needle),
            });
        }
    }
    let subtree_has_match: &HashSet<NodeId> = match &state.search_cache {
        Some(c) => &c.matches,
        None => &EMPTY_MATCHES,
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

// Borrowed when no search is active so the visible-row builder can
// uniformly take a `&HashSet<NodeId>` without forcing an empty
// allocation per call.
static EMPTY_MATCHES: std::sync::LazyLock<HashSet<NodeId>> = std::sync::LazyLock::new(HashSet::new);

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
            SortKey::Name => na.name_lower.cmp(&nb.name_lower),
            SortKey::FileCount => na.file_count.cmp(&nb.file_count),
            SortKey::DirCount => na.dir_count.cmp(&nb.dir_count),
            // Allocated/Mtime/Owner are optional; rows without a value
            // should always sort to the bottom regardless of direction.
            // The naive `Option::cmp` orders `None < Some(_)`, which means
            // flipping direction floats unknown rows to the top — rarely
            // what users want when sorting.
            SortKey::Allocated => {
                return cmp_option_none_last(&na.alloc_total, &nb.alloc_total, dir);
            }
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
///
/// `needle` must already be lowercased — the caller (`rebuild_visible_rows`)
/// lowercases the search input once per rebuild, and this function relies
/// on `Node::name_lower` for the haystack. That asymmetry is intentional:
/// it lets us avoid re-lowercasing every node's name on every keystroke.
fn compute_subtree_matches(tree: &Tree, needle: &str) -> HashSet<NodeId> {
    // Defend the contract in debug builds: a future caller that
    // forgets to lowercase the needle would otherwise produce a
    // silent "search returns nothing" bug, since `name_lower` only
    // ever matches lowercased input.
    debug_assert_eq!(
        needle,
        needle.to_lowercase(),
        "compute_subtree_matches: needle must be pre-lowercased",
    );
    let mut hits: HashSet<NodeId> = HashSet::new();
    for id in tree.iter_ids() {
        if let Some(n) = tree.get(id)
            && n.name_lower.contains(needle)
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
        Node::new_leaf(name, NodeKind::Dir, 0, Some(0), None, None)
    }
    fn file(name: &str, size: u64) -> Node {
        Node::new_leaf(name, NodeKind::File, size, Some(size), None, None)
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
    fn search_cache_is_reused_across_rebuilds_with_same_needle() {
        // Two rebuilds with the same needle and same tree should give
        // identical visible rows AND keep the same memoised match-set.
        // We can't observe "did we recompute?" directly without
        // instrumentation, so we snapshot the cache contents before
        // and after the second rebuild and assert they're identical
        // — combined with the row-for-row check that shape didn't
        // change either.
        let tree = sample();
        let mut state = UiState {
            search: "b1".into(),
            ..Default::default()
        };
        rebuild_visible_rows(&tree, &mut state);
        let cache_after_first = state
            .search_cache
            .as_ref()
            .map(|c| (c.needle.clone(), c.tree_len, c.matches.clone()));
        let rows_after_first: Vec<RowEntry> = state.visible_rows.clone();
        rebuild_visible_rows(&tree, &mut state);
        let cache_after_second = state
            .search_cache
            .as_ref()
            .map(|c| (c.needle.clone(), c.tree_len, c.matches.clone()));
        assert_eq!(cache_after_first, cache_after_second);
        assert_eq!(rows_after_first, state.visible_rows);
    }

    #[test]
    fn search_cache_invalidates_when_needle_changes() {
        let tree = sample();
        let mut state = UiState {
            search: "b1".into(),
            ..Default::default()
        };
        rebuild_visible_rows(&tree, &mut state);
        let first_matches = state
            .search_cache
            .as_ref()
            .expect("cache populated for non-empty search")
            .matches
            .clone();
        // Switch needles. The cache must rebuild against the new
        // needle, otherwise the visible rows would be stale.
        state.search = "alpha".into();
        rebuild_visible_rows(&tree, &mut state);
        let second_cache = state
            .search_cache
            .as_ref()
            .expect("cache repopulated after needle change");
        assert_eq!(second_cache.needle, "alpha");
        assert_ne!(second_cache.matches, first_matches);
    }

    #[test]
    fn search_cache_drops_when_search_cleared() {
        // Empty search means no filter, so we shouldn't be holding a
        // potentially large match-set on the side.
        let tree = sample();
        let mut state = UiState {
            search: "b1".into(),
            ..Default::default()
        };
        rebuild_visible_rows(&tree, &mut state);
        assert!(state.search_cache.is_some());
        state.search = String::new();
        rebuild_visible_rows(&tree, &mut state);
        assert!(state.search_cache.is_none());
    }

    #[test]
    fn invalidate_search_cache_clears_memo() {
        let tree = sample();
        let mut state = UiState {
            search: "b1".into(),
            ..Default::default()
        };
        rebuild_visible_rows(&tree, &mut state);
        assert!(state.search_cache.is_some());
        state.invalidate_search_cache();
        assert!(state.search_cache.is_none());
    }

    #[test]
    fn reset_for_new_scan_preserves_search_and_sort() {
        // Borrow a real NodeId from a sample tree — `NodeId`'s wrapped
        // u32 is private outside the scan module, and faking ids would
        // make this test brittle anyway.
        let tree = sample();
        let root = tree.root().unwrap();
        let mut state = UiState {
            search: "abc".into(),
            sort_key: SortKey::Name,
            sort_dir: SortDir::Asc,
            selected: Some(root),
            rows_dirty: true,
            ..Default::default()
        };
        state.expanded.insert(root);
        state.visible_rows.push(RowEntry { id: root, depth: 0 });
        state.reset_for_new_scan();
        assert_eq!(state.search, "abc");
        assert_eq!(state.sort_key, SortKey::Name);
        assert_eq!(state.sort_dir, SortDir::Asc);
        assert!(state.expanded.is_empty());
        assert_eq!(state.selected, None);
        assert!(state.visible_rows.is_empty());
        assert!(!state.rows_dirty);
        assert!(state.search_cache.is_none());
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
    fn status_line_surfaces_io_error_events_while_scanning() {
        use std::path::PathBuf;
        let progress = ScanProgress {
            entries: 100,
            bytes: 4096,
            errors: 7,
            current_path: PathBuf::from("/tmp/x"),
        };
        let s = status_line(&Status::Scanning, Some(&progress));
        assert!(s.contains("100 entries"), "got {s:?}");
        assert!(s.contains("7 i/o errors"), "got {s:?}");
    }

    #[test]
    fn status_line_surfaces_io_error_events_in_done_state() {
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
        assert!(s.contains("3 i/o errors"), "got {s:?}");
    }

    #[test]
    fn status_line_omits_error_text_when_no_errors() {
        use std::path::PathBuf;
        let progress = ScanProgress {
            entries: 100,
            bytes: 4096,
            errors: 0,
            current_path: PathBuf::from("/tmp/x"),
        };
        let s = status_line(&Status::Scanning, Some(&progress));
        assert!(!s.contains("i/o errors"), "got {s:?}");
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
                Some(10),
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
                Some(10),
                Some(UNIX_EPOCH + Duration::from_secs(2_000_000)),
                None,
            ),
        )
        .unwrap();
        // unknown mtime — was previously floated to the top under Asc and
        // bottom under Desc; should now stay at the bottom in both.
        t.insert(
            Some(r),
            Node::new_leaf("unknown", NodeKind::File, 10, Some(10), None, None),
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
                Some(10),
                None,
                Some("alice".into()),
            ),
        )
        .unwrap();
        t.insert(
            Some(r),
            Node::new_leaf(
                "bob_file",
                NodeKind::File,
                10,
                Some(10),
                None,
                Some("bob".into()),
            ),
        )
        .unwrap();
        t.insert(
            Some(r),
            Node::new_leaf("noowner_file", NodeKind::File, 10, Some(10), None, None),
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
