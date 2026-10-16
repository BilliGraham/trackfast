use eframe::egui::{self, Color32, RichText};

use crate::model::{Action, Screen, State};

const ACCENT: Color32 = Color32::from_rgb(208, 128, 200);

pub fn shell(ui: &mut egui::Ui, state: &State) -> Vec<Action> {
    let mut actions = Vec::new();
    egui::SidePanel::left("navigation")
        .exact_width(175.0)
        .resizable(false)
        .frame(
            egui::Frame::new()
                .fill(Color32::from_rgb(15, 15, 15))
                .inner_margin(18.0),
        )
        .show(ui, |ui| {
            ui.heading(RichText::new("Trackfast").color(ACCENT));
            ui.label("Desktop scaffold");
            ui.add_space(30.0);
            ui.weak("TRACK");
            if ui
                .selectable_label(state.screen == Screen::Timer, "Timer")
                .clicked()
            {
                actions.push(Action::Navigate(Screen::Timer));
            }
            ui.add_space(20.0);
            ui.weak("MANAGE");
            if ui
                .selectable_label(state.screen == Screen::Projects, "Projects")
                .clicked()
            {
                actions.push(Action::Navigate(Screen::Projects));
            }
            ui.add_space(30.0);
            ui.weak("Toggl is not connected.");
        });
    egui::CentralPanel::default().show(ui, |ui| {
        ui.add_space(12.0);
        match state.screen {
            Screen::Timer => timer(ui, &mut actions),
            Screen::Projects => projects(ui),
        }
    });
    actions
}

fn timer(ui: &mut egui::Ui, actions: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        ui.heading("What are you working on?");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Open widget").clicked() {
                actions.push(Action::OpenWidget);
            }
            ui.add_enabled(false, egui::Button::new("Start"));
            ui.monospace("0:00:00");
        });
    });
    ui.add_space(18.0);
    ui.separator();
    ui.add_space(18.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("List view").color(ACCENT).strong());
        ui.separator();
        ui.label("TODAY  0:00:00");
        ui.separator();
        ui.label("WEEK TOTAL  0:00:00");
    });
    ui.add_space(18.0);
    ui.separator();
    ui.add_space(16.0);
    ui.strong("Today");
    ui.add_space(42.0);
    ui.heading("Your time entries will appear here");
    ui.label("This scaffold has no timer data or account connection.");
    ui.add_space(10.0);
    ui.weak("Next steps: timer entry, project selection, and a list grouped by day.");
}

fn projects(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.heading("Projects");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_enabled(false, egui::Button::new("New project"));
        });
    });
    ui.add_space(20.0);
    ui.separator();
    ui.add_space(20.0);
    ui.heading("Project setup will go here");
    ui.label("Each project will have a name and a colour.");
    ui.label("Timer entries and widget tasks will use the same project selection.");
    ui.add_space(12.0);
    ui.weak("No projects are loaded or created in this scaffold.");
}

pub fn widget(ui: &mut egui::Ui) {
    ui.heading(RichText::new("Pinned tasks").color(ACCENT));
    ui.weak("This window stays above other windows.");
    ui.add_space(12.0);
    for slot in 1..=3 {
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label(format!("Task {slot}"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_enabled(false, egui::Button::new("Start"));
                    ui.monospace("0:00:00");
                });
            });
            ui.weak("No task selected");
        });
        ui.add_space(6.0);
    }
    ui.weak("Independent overlapping timers will follow in a later step.");
}
