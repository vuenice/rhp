mod app;
mod editor_panel;
mod modal;
mod models;
mod output_panel;
mod runner;
mod sidebar;
mod theme;

use app::RhpPlaygroundApp;
use eframe::egui;

fn main() -> eframe::Result {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("RHP Playground")
            .with_inner_size([1180.0, 720.0])
            .with_min_inner_size([700.0, 450.0])
            .with_active(true),
        ..Default::default()
    };

    eframe::run_native(
        "RHP Playground",
        native_options,
        Box::new(|cc| Ok(Box::new(RhpPlaygroundApp::new(cc)))),
    )
}
