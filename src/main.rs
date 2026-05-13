#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1024.0, 640.0])
            .with_min_inner_size([480.0, 320.0])
            .with_title("rustyTree"),
        ..Default::default()
    };

    eframe::run_native(
        "rustyTree",
        options,
        Box::new(|cc| Ok(Box::new(RustyTreeApp::new(cc)))),
    )
}

#[derive(Default)]
struct RustyTreeApp {}

impl RustyTreeApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self::default()
    }
}

impl eframe::App for RustyTreeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("toolbar").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("rustyTree");
                ui.separator();
                ui.label("disk-usage analyzer (scaffold)");
            });
        });

        egui::Panel::bottom("statusbar").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("ready");
            });
        });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(64.0);
                ui.heading("Welcome to rustyTree");
                ui.add_space(8.0);
                ui.label("Scan engine and tree view land in subsequent tasks.");
            });
        });
    }
}
