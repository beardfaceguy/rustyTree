//! Top-level eframe application: scan plumbing + per-frame poll loop.
//!
//! All shared front-end state (sort/search/expanded/visible-rows + the
//! status enum + column metadata) lives in `rustytree_core::view`. This
//! file owns only the eframe glue: the file picker, the per-frame poll,
//! and the layout that wires the toolbar / status bar / tree view.

use std::path::PathBuf;
use std::time::Duration;

use eframe::egui;
use rustytree_core::scan::{ScanEvent, ScanHandle, Tree, start_scan};
use rustytree_core::view::{ColumnKind, Status, UiState};

use crate::ui;

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
            self.status = Status::Error("no path entered — type one or click Browse".into());
            return;
        }
        self.tree = None;
        self.ui.reset_for_new_scan();
        match start_scan(path) {
            Ok(handle) => {
                self.status = Status::Scanning;
                self.scan = Some(handle);
            }
            Err(e) => {
                self.status = Status::Error(e.to_string());
            }
        }
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

/// Pixel widths for each column in the GUI. Kept here (rather than on
/// [`ColumnKind`] in core) because pixels are a GUI concept; the CLI sizes
/// columns in characters instead.
pub fn column_pixel_width(kind: ColumnKind) -> Option<f32> {
    match kind {
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
