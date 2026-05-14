//! Bottom status bar: shows scan progress, completion stats, or errors.

use eframe::egui;
use rustytree_core::view::status_line;

use crate::app::RustyTreeApp;

pub fn render(app: &RustyTreeApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let line = status_line(&app.status, app.ui.last_progress.as_ref());
        ui.label(line);

        if let Some(tree) = app.tree.as_ref() {
            ui.separator();
            let total_rows = app.ui.visible_rows.len();
            let total_nodes = tree.len();
            ui.label(format!("{} of {} rows visible", total_rows, total_nodes));
        }
    });
}
