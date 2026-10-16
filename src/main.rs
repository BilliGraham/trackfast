#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod model;
mod sync;
mod toggl;
mod ui;

use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1150.0, 720.0])
            .with_min_inner_size([760.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Trackfast",
        options,
        Box::new(|creation| {
            creation.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(app::App::new(creation.egui_ctx.clone())))
        }),
    )
}
