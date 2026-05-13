//! Top-level eframe application: scan plumbing + UI state.
//!
//! The render code itself lives in [`crate::ui`] so this file stays focused
//! on the lifecycle of a scan (start / poll / cancel) and the bag of UI
//! state the row renderer needs.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use eframe::egui;
use rustytree::format;
use rustytree::scan::{NodeId, ScanError, ScanEvent, ScanHandle, ScanProgress, Tree, start_scan};

use crate::ui;

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    #[default]
    Desc,
}

#[derive(Clone, Copy, Debug)]
pub struct RowEntry {
    pub id: NodeId,
    pub depth: u16,
}

/// Runtime status shown in the bottom status bar.
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

/// Mutable UI state that survives across frames but is recomputed eagerly
/// when sort/search/expansion changes.
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

pub struct RustyTreeApp {
    pub path_input: String,
    pub scan: Option<ScanHandle>,
    pub tree: Option<Tree>,
    pub status: Status,
    pub ui: UiState,
}

impl RustyTreeApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let cwd = std::env::current_dir()
            .ok()
            .and_then(|p| p.to_str().map(str::to_string))
            .unwrap_or_default();
        Self {
            path_input: cwd,
            scan: None,
            tree: None,
            status: Status::Idle,
            ui: UiState::default(),
        }
    }

    /// Pull every event currently waiting on the scan channel and update
    /// state accordingly.
    fn poll_scan(&mut self) {
        loop {
            let recv = match self.scan.as_ref() {
                Some(h) => h.try_recv(),
                None => return,
            };
            match recv {
                Ok(ScanEvent::Progress(p)) => {
                    self.ui.last_progress = Some(p);
                }
                Ok(ScanEvent::Done { tree, elapsed }) => {
                    let root = tree.root();
                    let (total_bytes, file_count, dir_count) = root
                        .and_then(|r| tree.get(r))
                        .map(|n| (n.size_total, n.file_count, n.dir_count))
                        .unwrap_or((0, 0, 0));
                    self.status = Status::Done {
                        elapsed,
                        total_bytes,
                        file_count,
                        dir_count,
                    };
                    if let Some(r) = root {
                        self.ui.expanded.clear();
                        self.ui.expanded.insert(r);
                    }
                    self.tree = Some(tree);
                    self.ui.rows_dirty = true;
                    self.scan = None;
                }
                Ok(ScanEvent::Cancelled) => {
                    self.status = Status::Cancelled;
                    self.scan = None;
                }
                Ok(ScanEvent::Error(e)) => {
                    self.status = Status::Error(format!("{e}"));
                    self.scan = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    if matches!(self.status, Status::Scanning) {
                        self.status = Status::Error(
                            "scan worker disconnected before reporting completion".into(),
                        );
                    }
                    self.scan = None;
                    break;
                }
            }
        }
    }

    pub fn start_scan_from_input(&mut self) {
        let path = PathBuf::from(self.path_input.trim());
        if path.as_os_str().is_empty() {
            self.status = Status::Error(ScanError::NotADirectory(path.clone()).to_string());
            return;
        }
        self.tree = None;
        self.ui = UiState {
            search: std::mem::take(&mut self.ui.search),
            sort_key: self.ui.sort_key,
            sort_dir: self.ui.sort_dir,
            ..Default::default()
        };
        self.status = Status::Scanning;
        self.scan = Some(start_scan(path));
    }

    pub fn pick_directory(&mut self) {
        if let Some(p) = rfd::FileDialog::new().pick_folder()
            && let Some(s) = p.to_str()
        {
            self.path_input = s.to_string();
        }
    }
}

impl eframe::App for RustyTreeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_scan();
        if self.scan.is_some() {
            // Keep repainting while a scan is in progress so progress updates
            // appear without requiring user input.
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }

        egui::Panel::top("toolbar").show_inside(ui, |ui| {
            ui::toolbar::render(self, ui);
        });

        egui::Panel::bottom("statusbar").show_inside(ui, |ui| {
            ui::status::render(self, ui);
        });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui::tree_view::render(self, ui);
        });
    }
}

/// Compose the columns that the table renders. Order here is the on-screen
/// left-to-right order. Kept alongside [`SortKey`] so headers and rows agree.
pub const COLUMNS: &[(&str, ColumnKind)] = &[
    ("Name", ColumnKind::Name),
    ("Size", ColumnKind::Size),
    ("%", ColumnKind::PercentOfRoot),
    ("Allocated", ColumnKind::Allocated),
    ("Files", ColumnKind::FileCount),
    ("Dirs", ColumnKind::DirCount),
    ("Modified", ColumnKind::Mtime),
    ("Owner", ColumnKind::Owner),
];

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
    /// Pixel width allocated to this column. Name is "rest"; everything else
    /// is a fixed width chosen to fit typical content.
    pub fn width(self) -> Option<f32> {
        match self {
            ColumnKind::Name => None,
            ColumnKind::Size => Some(96.0),
            ColumnKind::PercentOfRoot => Some(72.0),
            ColumnKind::Allocated => Some(96.0),
            ColumnKind::FileCount => Some(64.0),
            ColumnKind::DirCount => Some(64.0),
            ColumnKind::Mtime => Some(140.0),
            ColumnKind::Owner => Some(110.0),
        }
    }

    /// Sort key triggered when the user clicks this column's header.
    pub fn sort_key(self) -> Option<SortKey> {
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

/// Format the current status as a single line for the bottom bar.
pub fn status_line(status: &Status, last_progress: Option<&ScanProgress>) -> String {
    match status {
        Status::Idle => "ready".into(),
        Status::Scanning => match last_progress {
            Some(p) => format!(
                "scanning... {} entries, {} so far ({})",
                p.entries,
                format::bytes(p.bytes),
                p.current_path.display()
            ),
            None => "scanning...".into(),
        },
        Status::Done {
            elapsed,
            total_bytes,
            file_count,
            dir_count,
        } => format!(
            "done in {} | {} | {} files, {} dirs",
            format::elapsed(*elapsed),
            format::bytes(*total_bytes),
            file_count,
            dir_count
        ),
        Status::Cancelled => "cancelled".into(),
        Status::Error(e) => format!("error: {e}"),
    }
}
