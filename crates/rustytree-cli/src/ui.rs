//! Ratatui rendering for the CLI.
//!
//! The render path is: split the frame into header / column-header /
//! body / footer / hints, then write directly into the buffer for the
//! body so we get fine-grained control over column alignment.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use rustytree_core::format;
use rustytree_core::scan::{Node, Tree};
use rustytree_core::view::{
    COLUMNS, ColumnKind, RowEntry, SortDir, UiState, chevron_glyph, status_line,
};

use crate::app::{Mode, RustyTreeApp};

/// Column widths in characters. The Name column is None — it absorbs
/// whatever space is left after the fixed-width columns are subtracted.
fn column_char_width(kind: ColumnKind) -> Option<u16> {
    match kind {
        ColumnKind::Name => None,
        ColumnKind::Size => Some(12),
        ColumnKind::PercentOfRoot => Some(7),
        ColumnKind::Allocated => Some(12),
        ColumnKind::FileCount => Some(8),
        ColumnKind::DirCount => Some(7),
        ColumnKind::Mtime => Some(17),
        ColumnKind::Owner => Some(12),
    }
}

const COL_GAP: u16 = 1;

pub fn render(frame: &mut Frame, app: &mut RustyTreeApp) {
    let area = frame.area();

    // Vertical split. Search bar only takes a row when Search mode is
    // active; otherwise its constraint is Length(0) and ratatui hides it.
    let search_height = if app.mode == Mode::Search { 1 } else { 0 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // header (path + status)
            Constraint::Length(1), // column header
            Constraint::Min(1),    // body
            Constraint::Length(search_height),
            Constraint::Length(1), // hints / footer
        ])
        .split(area);

    render_header(frame, chunks[0], app);
    render_column_header(frame, chunks[1], &app.ui);
    render_body(frame, chunks[2], app);
    if search_height > 0 {
        render_search_bar(frame, chunks[3], &app.ui.search);
    }
    render_footer(frame, chunks[4], app);

    if app.help_open {
        render_help_overlay(frame, area);
    }
}

fn render_header(frame: &mut Frame, area: Rect, app: &RustyTreeApp) {
    let title = Line::from(vec![
        Span::styled("rustyTree", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw("  "),
        Span::raw(app.path.display().to_string()),
    ]);
    let status = Line::from(Span::styled(
        status_line(&app.status, app.ui.last_progress.as_ref()),
        Style::default().fg(Color::Cyan),
    ));
    let para = Paragraph::new(vec![title, status]);
    frame.render_widget(para, area);
}

fn render_column_header(frame: &mut Frame, area: Rect, state: &UiState) {
    if area.width == 0 {
        return;
    }
    let widths = compute_widths(area.width);
    let mut spans: Vec<Span> = Vec::with_capacity(COLUMNS.len() * 2);
    for (i, kind) in COLUMNS.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        let label = kind.label();
        let active = kind
            .sort_key()
            .map(|k| k == state.sort_key)
            .unwrap_or(false);
        let arrow = if active {
            match state.sort_dir {
                SortDir::Asc => " ^",
                SortDir::Desc => " v",
            }
        } else {
            ""
        };
        let text = format!("{label}{arrow}");
        let cell = pad_to_width(&text, widths[i] as usize, *kind);
        let style = if active {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().add_modifier(Modifier::BOLD)
        };
        spans.push(Span::styled(cell, style));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_body(frame: &mut Frame, area: Rect, app: &mut RustyTreeApp) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let Some(tree) = app.tree.as_ref() else {
        render_empty_state(frame, area);
        return;
    };

    let widths = compute_widths(area.width);
    let body_height = area.height as usize;
    let total_rows = app.ui.visible_rows.len();

    // Keep the selected row in view by adjusting scroll_offset when the
    // selection moves outside [offset, offset + body_height).
    if let Some(selected_idx) = app
        .ui
        .selected
        .and_then(|id| app.ui.visible_rows.iter().position(|r| r.id == id))
    {
        if selected_idx < app.scroll_offset {
            app.scroll_offset = selected_idx;
        } else if selected_idx >= app.scroll_offset + body_height {
            app.scroll_offset = selected_idx + 1 - body_height;
        }
    }
    let max_offset = total_rows.saturating_sub(body_height);
    if app.scroll_offset > max_offset {
        app.scroll_offset = max_offset;
    }

    let root_total = tree
        .root()
        .and_then(|r| tree.get(r))
        .map(|n| n.size_total)
        .unwrap_or(0);

    let visible = app
        .ui
        .visible_rows
        .iter()
        .skip(app.scroll_offset)
        .take(body_height);

    let mut lines: Vec<Line> = Vec::with_capacity(body_height);
    for row in visible {
        let line = build_row_line(tree, *row, &app.ui, &widths, root_total);
        lines.push(line);
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn build_row_line(
    tree: &Tree,
    row: RowEntry,
    state: &UiState,
    widths: &[u16],
    root_total: u64,
) -> Line<'static> {
    let Some(node) = tree.get(row.id) else {
        return Line::from("");
    };
    let selected = state.selected == Some(row.id);
    let row_style = if selected {
        Style::default()
            .bg(Color::Indexed(238))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    let expanded = state.expanded.contains(&row.id);
    let chevron = chevron_glyph(!node.children.is_empty(), expanded);

    let mut spans: Vec<Span<'static>> = Vec::new();
    for (i, kind) in COLUMNS.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" ", row_style));
        }
        let cell = format_cell(node, *kind, row.depth, chevron, widths[i], root_total);
        let cell_style = match kind {
            ColumnKind::Name => row_style,
            ColumnKind::PercentOfRoot => {
                let frac = if root_total == 0 {
                    0.0
                } else {
                    node.size_total as f32 / root_total as f32
                };
                if frac > 0.5 {
                    row_style.fg(Color::Red)
                } else if frac > 0.1 {
                    row_style.fg(Color::Yellow)
                } else {
                    row_style
                }
            }
            _ => row_style,
        };
        spans.push(Span::styled(cell, cell_style));
    }
    Line::from(spans)
}

fn format_cell(
    node: &Node,
    kind: ColumnKind,
    depth: u16,
    chevron: &'static str,
    width: u16,
    root_total: u64,
) -> String {
    match kind {
        ColumnKind::Name => format_name_cell(node, depth, chevron, width as usize),
        ColumnKind::Size => pad_to_width(&format::bytes(node.size_total), width as usize, kind),
        ColumnKind::PercentOfRoot => {
            let frac = if root_total == 0 {
                0.0
            } else {
                node.size_total as f32 / root_total as f32
            };
            pad_to_width(&format::percent(frac), width as usize, kind)
        }
        ColumnKind::Allocated => {
            pad_to_width(&format::bytes(node.alloc_total), width as usize, kind)
        }
        ColumnKind::FileCount => pad_to_width(&node.file_count.to_string(), width as usize, kind),
        ColumnKind::DirCount => pad_to_width(&node.dir_count.to_string(), width as usize, kind),
        ColumnKind::Mtime => pad_to_width(&format::mtime(node.mtime), width as usize, kind),
        ColumnKind::Owner => {
            pad_to_width(node.owner.as_deref().unwrap_or(""), width as usize, kind)
        }
    }
}

/// Build the "Name" cell: depth-based indent, expand/collapse glyph, and
/// the node name (truncated with an ellipsis if the column is too narrow
/// to fit it). The chevron is decided at the call site because it needs
/// the row id, not just the node.
fn format_name_cell(node: &Node, depth: u16, chevron: &'static str, width: usize) -> String {
    let indent: String = "  ".repeat(depth as usize);
    let prefix_len = indent.chars().count() + chevron.chars().count();
    let max_name = width.saturating_sub(prefix_len);
    let name_chars = node.name.chars().count();
    let name = if name_chars > max_name && max_name >= 1 {
        let mut truncated: String = node.name.chars().take(max_name.saturating_sub(1)).collect();
        truncated.push('\u{2026}');
        truncated
    } else {
        node.name.clone()
    };
    let combined = format!("{indent}{chevron}{name}");
    pad_to_width(&combined, width, ColumnKind::Name)
}

fn render_empty_state(frame: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Welcome to rustyTree",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("Press `s` to scan the configured path, or `?` for help."),
    ];
    let para = Paragraph::new(lines).alignment(ratatui::layout::Alignment::Center);
    frame.render_widget(para, area);
}

fn render_search_bar(frame: &mut Frame, area: Rect, query: &str) {
    let line = Line::from(vec![
        Span::styled("/", Style::default().fg(Color::Yellow)),
        Span::raw(query.to_string()),
        Span::styled("_", Style::default().fg(Color::Yellow)),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn render_footer(frame: &mut Frame, area: Rect, app: &RustyTreeApp) {
    let visible = app.ui.visible_rows.len();
    let total = app.tree.as_ref().map(|t| t.len()).unwrap_or(0);
    let count = if total > 0 {
        format!("{visible}/{total}")
    } else {
        "-".into()
    };

    let hints = match app.mode {
        Mode::Search => "Enter apply  Esc cancel".to_string(),
        Mode::Normal => format!(
            "{count}  q quit  s/r scan  Esc cancel  /search c clear  1-7 sort  hjkl/arrows  ?help"
        ),
    };
    let para = Paragraph::new(Line::from(Span::styled(
        hints,
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(para, area);
}

fn render_help_overlay(frame: &mut Frame, area: Rect) {
    let popup_w = area.width.min(60);
    let popup_h = area.height.min(18);
    let x = area.x + (area.width.saturating_sub(popup_w)) / 2;
    let y = area.y + (area.height.saturating_sub(popup_h)) / 2;
    let rect = Rect::new(x, y, popup_w, popup_h);

    frame.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" rustyTree help (? to close) ");
    let body = vec![
        Line::from(vec![
            Span::styled("  q  ", Style::default().fg(Color::Yellow)),
            Span::raw("quit"),
        ]),
        Line::from(vec![
            Span::styled("  s/r  ", Style::default().fg(Color::Yellow)),
            Span::raw("start/restart scan on current path"),
        ]),
        Line::from(vec![
            Span::styled("  Esc  ", Style::default().fg(Color::Yellow)),
            Span::raw("cancel running scan"),
        ]),
        Line::from(vec![
            Span::styled("  Up/Down  ", Style::default().fg(Color::Yellow)),
            Span::raw("move selection (j/k also)"),
        ]),
        Line::from(vec![
            Span::styled("  PgUp/PgDn  ", Style::default().fg(Color::Yellow)),
            Span::raw("page up/down"),
        ]),
        Line::from(vec![
            Span::styled("  g/G  ", Style::default().fg(Color::Yellow)),
            Span::raw("first/last row"),
        ]),
        Line::from(vec![
            Span::styled("  Enter / Right / l  ", Style::default().fg(Color::Yellow)),
            Span::raw("expand"),
        ]),
        Line::from(vec![
            Span::styled("  Left / h  ", Style::default().fg(Color::Yellow)),
            Span::raw("collapse / step out"),
        ]),
        Line::from(vec![
            Span::styled("  1..7  ", Style::default().fg(Color::Yellow)),
            Span::raw("sort: Name Size Allocated Files Dirs Modified Owner"),
        ]),
        Line::from(vec![
            Span::styled("  /  ", Style::default().fg(Color::Yellow)),
            Span::raw("search; Enter apply, Esc abort"),
        ]),
        Line::from(vec![
            Span::styled("  c  ", Style::default().fg(Color::Yellow)),
            Span::raw("clear search"),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl+C  ", Style::default().fg(Color::Yellow)),
            Span::raw("force quit"),
        ]),
    ];
    let para = Paragraph::new(body).block(block);
    frame.render_widget(para, rect);
}

/// Compute character widths for each column given the available terminal
/// width. The Name column gets whatever's left after the fixed-width
/// columns are subtracted.
fn compute_widths(total: u16) -> Vec<u16> {
    let mut fixed = 0u16;
    let mut name_idx = 0;
    let mut widths = vec![0u16; COLUMNS.len()];
    for (i, kind) in COLUMNS.iter().enumerate() {
        match column_char_width(*kind) {
            Some(w) => {
                widths[i] = w;
                fixed += w;
            }
            None => name_idx = i,
        }
    }
    let gaps = COL_GAP * (COLUMNS.len() as u16 - 1);
    let name_w = total.saturating_sub(fixed + gaps).max(10);
    widths[name_idx] = name_w;
    widths
}

/// Pad `text` to exactly `width` characters: numeric columns get the
/// padding on the **left** so digits right-align under each other, text
/// columns (Name, Owner, Mtime) get the padding on the **right** so they
/// left-align flush with the column header. Values longer than `width` are
/// truncated with an ellipsis.
fn pad_to_width(text: &str, width: usize, kind: ColumnKind) -> String {
    let n = text.chars().count();
    if n == width {
        return text.to_string();
    }
    if n > width {
        if width <= 1 {
            return text.chars().take(width).collect();
        }
        return text
            .chars()
            .take(width - 1)
            .chain(std::iter::once('\u{2026}'))
            .collect();
    }
    let pad = " ".repeat(width - n);
    match kind {
        ColumnKind::Name | ColumnKind::Owner | ColumnKind::Mtime => format!("{text}{pad}"),
        _ => format!("{pad}{text}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_widths_assigns_remaining_to_name() {
        let widths = compute_widths(120);
        let total: u16 = widths.iter().sum();
        let gaps = COL_GAP * (COLUMNS.len() as u16 - 1);
        assert_eq!(total + gaps, 120);
    }

    #[test]
    fn compute_widths_keeps_name_at_minimum_when_terminal_is_tiny() {
        let widths = compute_widths(20);
        let name_idx = COLUMNS.iter().position(|c| *c == ColumnKind::Name).unwrap();
        assert!(widths[name_idx] >= 10);
    }

    #[test]
    fn pad_to_width_left_pads_numeric_columns() {
        let out = pad_to_width("42", 6, ColumnKind::FileCount);
        assert_eq!(out, "    42");
    }

    #[test]
    fn pad_to_width_right_pads_text_columns() {
        let out = pad_to_width("alice", 8, ColumnKind::Owner);
        assert_eq!(out, "alice   ");
    }

    #[test]
    fn pad_to_width_truncates_with_ellipsis() {
        let out = pad_to_width("very-long-value", 8, ColumnKind::Owner);
        assert_eq!(out.chars().count(), 8);
        assert!(out.ends_with('\u{2026}'));
    }
}
