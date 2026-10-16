use eframe::egui;

use crate::model::{Action, State};
use crate::ui;

#[derive(Default)]
pub struct App {
    state: State,
}

impl App {
    fn apply(&mut self, action: Action) {
        match action {
            Action::Navigate(screen) => self.state.screen = screen,
            Action::OpenWidget => self.state.widget_open = true,
            Action::CloseWidget => self.state.widget_open = false,
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        for action in ui::shell(ui, &self.state) {
            self.apply(action);
        }
        if self.state.widget_open {
            let mut close = false;
            ui.ctx().show_viewport_immediate(
                egui::ViewportId::from_hash_of("task-widget"),
                egui::ViewportBuilder::default()
                    .with_title("Trackfast widget")
                    .with_inner_size([350.0, 280.0])
                    .with_min_inner_size([300.0, 250.0])
                    .with_window_level(egui::WindowLevel::AlwaysOnTop),
                |widget_ui, _class| {
                    close = widget_ui.input(|input| input.viewport().close_requested());
                    egui::CentralPanel::default().show(widget_ui, ui::widget);
                },
            );
            if close {
                self.apply(Action::CloseWidget);
            }
        }
    }
}
