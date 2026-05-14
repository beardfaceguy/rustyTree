//! Hierarchical tree-of-folders view with virtualized rendering.
//!
//! - Children at each level are sorted by the current [`SortKey`]/[`SortDir`].
//! - Search filters out subtrees whose names contain no matches; matched
//!   subtrees auto-expand so the matches are visible.
//! - Only rows in the visible scroll window get rendered, so the cost is
//!   independent of total tree size.

use std::collections::HashSet;

use eframe::egui;
use rustytree::format;
use rustytree::scan::{NodeId, Tree};

use crate::app::{COLUMNS, ColumnKind, RowEntry, RustyTreeApp, SortDir, SortKey};

const ROW_HEIGHT: f32 = 22.0;
const INDENT_PER_DEPTH: f32 = 16.0;

pub fn render(app: &mut RustyTreeApp, ui: &mut egui::Ui) {
    // Split borrows so the closure inside `show_rows` can mutate `state`
    // while `tree` stays immutably borrowed.
    let RustyTreeApp {
        tree, ui: state, ..
    } = app;
    let Some(tree) = tree.as_ref() else {
        empty_state(ui);
        return;
    };

    if state.rows_dirty {
        rebuild_visible_rows(tree, state);
        state.rows_dirty = false;
    }

    render_header(state, ui);
    ui.separator();

    let rows = state.visible_rows.clone();
    let total_rows = rows.len();
    let root_total = tree
        .root()
        .and_then(|r| tree.get(r))
        .map(|n| n.size_total)
        .unwrap_or(0);

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, ROW_HEIGHT, total_rows, |ui, range| {
            for row_idx in range {
                let row = rows[row_idx];
                let mut clicked_chevron = false;
                let mut clicked_row = false;
                render_row(
                    ui,
                    tree,
                    row,
                    &mut clicked_chevron,
                    &mut clicked_row,
                    state.expanded.contains(&row.id),
                    state.selected == Some(row.id),
                    root_total,
                );
                if clicked_chevron {
                    toggle_expand(&mut state.expanded, row.id);
                    state.rows_dirty = true;
                }
                if clicked_row {
                    state.selected = Some(row.id);
                }
            }
        });
}

fn empty_state(ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(48.0);
        ui.heading("Welcome to rustyTree");
        ui.add_space(8.0);
        ui.label("Pick a directory above and click Scan to build a size tree.");
    });
}

fn render_header(state: &mut crate::app::UiState, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        for (label, kind) in COLUMNS {
            let total_avail = ui.available_size().x;
            let width = match kind.width() {
                Some(w) => w,
                None => total_avail.max(120.0),
            };

            let active = kind
                .sort_key()
                .map(|k| k == state.sort_key)
                .unwrap_or(false);
            let arrow = if active {
                match state.sort_dir {
                    SortDir::Asc => " \u{2191}",
                    SortDir::Desc => " \u{2193}",
                }
            } else {
                ""
            };
            let text = format!("{label}{arrow}");
            let resp = ui.allocate_ui_with_layout(
                egui::vec2(width, ROW_HEIGHT),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    let btn = egui::Button::new(text).frame(false);
                    ui.add_sized([width, ROW_HEIGHT], btn)
                },
            );
            if resp.inner.clicked()
                && let Some(new_key) = kind.sort_key()
            {
                if state.sort_key == new_key {
                    state.sort_dir = match state.sort_dir {
                        SortDir::Asc => SortDir::Desc,
                        SortDir::Desc => SortDir::Asc,
                    };
                } else {
                    state.sort_key = new_key;
                    state.sort_dir = match new_key {
                        SortKey::Name | SortKey::Owner => SortDir::Asc,
                        _ => SortDir::Desc,
                    };
                }
                state.rows_dirty = true;
            }
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn render_row(
    ui: &mut egui::Ui,
    tree: &Tree,
    row: RowEntry,
    clicked_chevron: &mut bool,
    clicked_row: &mut bool,
    expanded: bool,
    selected: bool,
    root_total: u64,
) {
    let Some(node) = tree.get(row.id) else { return };
    let row_bg = if selected {
        Some(ui.visuals().selection.bg_fill)
    } else {
        None
    };

    let row_resp = ui.scope(|ui| {
        if let Some(bg) = row_bg {
            let rect = ui.max_rect();
            ui.painter().rect_filled(rect, 0.0, bg);
        }
        ui.horizontal(|ui| {
            for (_, kind) in COLUMNS {
                let total_avail = ui.available_size().x;
                let width = match kind.width() {
                    Some(w) => w,
                    None => total_avail.max(120.0),
                };
                ui.allocate_ui_with_layout(
                    egui::vec2(width, ROW_HEIGHT),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| match kind {
                        ColumnKind::Name => {
                            render_name_cell(ui, node, row.depth, expanded, width, clicked_chevron);
                        }
                        ColumnKind::Size => {
                            ui.label(format::bytes(node.size_total));
                        }
                        ColumnKind::PercentOfRoot => {
                            render_percent_cell(ui, node.size_total, root_total, width);
                        }
                        ColumnKind::Allocated => {
                            ui.label(format::bytes(node.alloc_total));
                        }
                        ColumnKind::FileCount => {
                            ui.label(format!("{}", node.file_count));
                        }
                        ColumnKind::DirCount => {
                            ui.label(format!("{}", node.dir_count));
                        }
                        ColumnKind::Mtime => {
                            ui.label(format::mtime(node.mtime));
                        }
                        ColumnKind::Owner => {
                            ui.label(node.owner.as_deref().unwrap_or(""));
                        }
                    },
                );
            }
        })
    });

    if row_resp.response.interact(egui::Sense::click()).clicked() {
        *clicked_row = true;
    }
}

fn render_name_cell(
    ui: &mut egui::Ui,
    node: &rustytree::scan::Node,
    depth: u16,
    expanded: bool,
    width: f32,
    clicked_chevron: &mut bool,
) {
    let indent = INDENT_PER_DEPTH * depth as f32;
    ui.add_space(indent);

    let has_children = !node.children.is_empty();
    let chevron = chevron_glyph(has_children, expanded);

    let chevron_btn = egui::Button::new(chevron).frame(false);
    let resp = ui.add_enabled(has_children, chevron_btn);
    if resp.clicked() {
        *clicked_chevron = true;
    }

    let remaining = (width - indent - 24.0).max(40.0);
    let label = egui::Label::new(node.name.clone()).truncate();
    ui.allocate_ui_with_layout(
        egui::vec2(remaining, ROW_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.add(label);
        },
    );
}

/// Pick the chevron glyph for a row.
///
/// Pure ASCII so it always renders in egui's embedded font; previously this
/// used U+25B8 / U+25BE which fell back to `.notdef` boxes ("tofu") on
/// systems whose default font lacked those glyphs. The two-space variants
/// keep column alignment consistent across rows with and without a toggle.
fn chevron_glyph(has_children: bool, expanded: bool) -> &'static str {
    match (has_children, expanded) {
        (false, _) => "  ",
        (true, true) => "- ",
        (true, false) => "+ ",
    }
}

fn render_percent_cell(ui: &mut egui::Ui, size_total: u64, root_total: u64, width: f32) {
    let frac = if root_total == 0 {
        0.0
    } else {
        size_total as f32 / root_total as f32
    };
    let frac = frac.clamp(0.0, 1.0);

    let bar_w = (width - 56.0).max(20.0);
    let (rect, _resp) = ui.allocate_exact_size(egui::vec2(bar_w, 8.0), egui::Sense::hover());
    let painter = ui.painter();
    let bg = ui.visuals().widgets.inactive.bg_fill;
    let fg = ui.visuals().selection.bg_fill;
    painter.rect_filled(rect, 2.0, bg);
    let mut filled = rect;
    filled.set_width(rect.width() * frac);
    painter.rect_filled(filled, 2.0, fg);

    ui.label(format::percent(frac));
}

fn toggle_expand(expanded: &mut HashSet<NodeId>, id: NodeId) {
    if !expanded.remove(&id) {
        expanded.insert(id);
    }
}

/// Re-flatten the tree into the visible-rows list according to the current
/// expansion / sort / search state. Auto-expands ancestors of any nodes
/// matching the search query.
pub fn rebuild_visible_rows(tree: &Tree, state: &mut crate::app::UiState) {
    state.visible_rows.clear();
    let Some(root) = tree.root() else { return };

    let needle = state.search.trim().to_lowercase();
    let filter_active = !needle.is_empty();
    let subtree_has_match = if filter_active {
        compute_subtree_matches(tree, &needle)
    } else {
        HashSet::new()
    };
    if filter_active {
        for id in tree.iter_ids() {
            if subtree_has_match.contains(&id) {
                state.expanded.insert(id);
            }
        }
    }

    let mut stack: Vec<(NodeId, u16)> = vec![(root, 0)];
    while let Some((id, depth)) = stack.pop() {
        if filter_active && !subtree_has_match.contains(&id) {
            continue;
        }
        state.visible_rows.push(RowEntry { id, depth });
        if state.expanded.contains(&id)
            && let Some(node) = tree.get(id)
        {
            let mut children = node.children.clone();
            sort_children(&mut children, tree, state.sort_key, state.sort_dir);
            for c in children.iter().rev() {
                stack.push((*c, depth + 1));
            }
        }
    }
}

fn sort_children(children: &mut [NodeId], tree: &Tree, key: SortKey, dir: SortDir) {
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
            SortKey::Mtime => na.mtime.cmp(&nb.mtime),
            SortKey::Owner => na.owner.cmp(&nb.owner),
        };
        match dir {
            SortDir::Asc => ord,
            SortDir::Desc => ord.reverse(),
        }
    });
}

/// Compute the set of node ids whose subtree contains at least one node
/// whose lowercased name contains `needle`. Includes the matching nodes
/// themselves and all of their ancestors up to the root.
fn compute_subtree_matches(tree: &Tree, needle: &str) -> HashSet<NodeId> {
    // Direct matches.
    let mut hits: HashSet<NodeId> = HashSet::new();
    for id in tree.iter_ids() {
        if let Some(n) = tree.get(id)
            && n.name.to_lowercase().contains(needle)
        {
            hits.insert(id);
        }
    }
    // Walk up adding ancestors.
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
    use rustytree::scan::{Node, NodeKind};

    fn dir(name: &str) -> Node {
        Node::new_leaf(name, NodeKind::Dir, 0, 0, None, None)
    }
    fn file(name: &str, size: u64) -> Node {
        Node::new_leaf(name, NodeKind::File, size, size, None, None)
    }

    fn sample() -> Tree {
        let mut t = Tree::new();
        let r = t.insert(None, dir("root"));
        let a = t.insert(Some(r), dir("alpha"));
        t.insert(Some(a), file("a1.bin", 100));
        t.insert(Some(a), file("a2.txt", 200));
        let b = t.insert(Some(r), dir("beta"));
        t.insert(Some(b), file("b1.bin", 50));
        t.insert(Some(r), file("c.bin", 1000));
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
        let mut state = crate::app::UiState::default();
        rebuild_visible_rows(&tree, &mut state);
        assert_eq!(state.visible_rows.len(), 1);
    }

    #[test]
    fn flatten_expanded_root_shows_root_plus_children() {
        let tree = sample();
        let mut state = crate::app::UiState::default();
        state.expanded.insert(tree.root().unwrap());
        rebuild_visible_rows(&tree, &mut state);
        // root + alpha + beta + c.bin = 4
        assert_eq!(state.visible_rows.len(), 4);
    }

    #[test]
    fn search_filters_to_matching_subtrees_and_auto_expands() {
        let tree = sample();
        let mut state = crate::app::UiState {
            search: "b1".into(),
            ..Default::default()
        };
        rebuild_visible_rows(&tree, &mut state);
        // Should show root + beta + b1.bin (auto-expanded ancestor chain)
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
    fn search_is_case_insensitive() {
        let tree = sample();
        let mut state = crate::app::UiState {
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
        let mut state = crate::app::UiState {
            sort_key: SortKey::Name,
            sort_dir: SortDir::Asc,
            ..Default::default()
        };
        state.expanded.insert(tree.root().unwrap());
        rebuild_visible_rows(&tree, &mut state);
        // First child of root in ascending name order: "alpha", "beta", "c.bin"
        let names: Vec<&str> = state
            .visible_rows
            .iter()
            .skip(1) // skip root itself
            .map(|r| tree.get(r.id).unwrap().name.as_str())
            .collect();
        assert_eq!(names, vec!["alpha", "beta", "c.bin"]);
    }

    #[test]
    fn sort_by_size_descending_is_default() {
        let tree = sample();
        let mut state = crate::app::UiState::default();
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
}
