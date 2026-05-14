#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod ui;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 720.0])
            .with_min_inner_size([640.0, 400.0])
            .with_title("rustyTree"),
        ..Default::default()
    };

    eframe::run_native(
        "rustyTree",
        options,
        Box::new(|cc| Ok(Box::new(app::RustyTreeApp::new(cc)))),
    )
}
