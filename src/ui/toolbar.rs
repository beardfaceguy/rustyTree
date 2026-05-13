//! Top toolbar: path input, file picker, scan/cancel, search box.

use eframe::egui;

use crate::app::{RustyTreeApp, Status};

pub fn render(app: &mut RustyTreeApp, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        ui.heading("rustyTree");
        ui.separator();

        ui.label("Path:");
        let response = ui.add(
            egui::TextEdit::singleline(&mut app.path_input)
                .desired_width(360.0)
                .hint_text("/path/to/directory"),
        );
        let enter_pressed = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

        if ui.button("Browse...").clicked() {
            app.pick_directory();
        }

        let scanning = matches!(app.status, Status::Scanning);
        let scan_label = if scanning { "Rescan" } else { "Scan" };

        let scan_clicked = ui
            .add_enabled(!scanning, egui::Button::new(scan_label))
            .clicked();
        if scan_clicked || enter_pressed {
            app.start_scan_from_input();
        }

        if ui
            .add_enabled(scanning, egui::Button::new("Cancel"))
            .clicked()
            && let Some(handle) = app.scan.as_ref()
        {
            handle.cancel();
        }

        ui.separator();

        ui.label("Search:");
        let search_resp = ui.add(
            egui::TextEdit::singleline(&mut app.ui.search)
                .desired_width(220.0)
                .hint_text("filter by name (case-insensitive)"),
        );
        if search_resp.changed() {
            app.ui.rows_dirty = true;
        }
        if !app.ui.search.is_empty() && ui.button("Clear").clicked() {
            app.ui.search.clear();
            app.ui.rows_dirty = true;
        }
    });
}
