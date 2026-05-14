//! Hierarchical tree-of-folders view with virtualized rendering.
//!
//! All sort/search/flatten logic lives in `rustytree_core::view`; this
//! module is purely the egui-side renderer (header row + virtualized body
//! rows + the size-bar widget). The CLI uses the same core helpers to
//! drive its ratatui-based table.

use eframe::egui;
use rustytree_core::format;
use rustytree_core::scan::Tree;
use rustytree_core::view::{
    COLUMNS, ColumnKind, RowEntry, SortDir, SortKey, UiState, chevron_glyph, rebuild_visible_rows,
    toggle_expand,
};

use crate::app::{RustyTreeApp, column_pixel_width};

const ROW_HEIGHT: f32 = 22.0;
const INDENT_PER_DEPTH: f32 = 16.0;

pub fn render(app: &mut RustyTreeApp, ui: &mut egui::Ui) {
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

fn render_header(state: &mut UiState, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        for kind in COLUMNS {
            let total_avail = ui.available_size().x;
            let width = match column_pixel_width(*kind) {
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
            let text = format!("{}{arrow}", kind.label());
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
            for kind in COLUMNS {
                let total_avail = ui.available_size().x;
                let width = match column_pixel_width(*kind) {
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
    node: &rustytree_core::scan::Node,
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
