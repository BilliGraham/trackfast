use eframe::egui::{self, RichText};

use super::{dot, project_colour, style};
use crate::db::ProjectDraft;
use crate::entries::{Project, Snapshot};
use crate::model::{Action, Drafts, PALETTE, State};

pub fn show(
    ui: &mut egui::Ui,
    state: &State,
    data: &Snapshot,
    drafts: &mut Drafts,
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        ui.heading("Projects");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("New project").clicked() {
                actions.push(Action::EditProject(None));
            }
        });
    });
    super::problem(ui, state);
    ui.add_space(12.0);
    if let Some(draft) = &mut drafts.project {
        editor(ui, data, draft, actions);
        ui.add_space(12.0);
    }
    ui.separator();
    if data.projects.is_empty() {
        ui.add_space(20.0);
        ui.label(RichText::new("No projects").size(16.0));
        ui.label("Create a project to group your tasks and give them a colour.");
        return;
    }
    let mut projects: Vec<&Project> = data.projects.iter().collect();
    projects.sort_by_key(|project| (project.archived, project.name.to_lowercase()));
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            for project in projects {
                ui.push_id(project.id, |ui| row(ui, data, project, actions));
            }
        });
}

fn row(ui: &mut egui::Ui, data: &Snapshot, project: &Project, actions: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        dot(ui, project_colour(&project.colour));
        ui.label(&project.name);
        if let Some(client) = project.client_id.and_then(|id| data.client(id)) {
            ui.weak(&client.name);
        }
        if project.archived {
            ui.weak("archived");
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (label, archived) = if project.archived {
                ("Restore", false)
            } else {
                ("Archive", true)
            };
            if ui.button(label).clicked() {
                actions.push(Action::SetArchived(project.id, archived));
            }
            if ui.button("Edit").clicked() {
                actions.push(Action::EditProject(Some(project.id)));
            }
        });
    });
}

fn editor(ui: &mut egui::Ui, data: &Snapshot, draft: &mut ProjectDraft, actions: &mut Vec<Action>) {
    style::card().inner_margin(16).show(ui, |ui| {
        ui.set_max_width(460.0);
        ui.label(
            RichText::new(if draft.id.is_some() {
                "Edit project"
            } else {
                "New project"
            })
            .size(15.0)
            .strong(),
        );
        egui::Grid::new("project-editor")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("Name");
                ui.add(egui::TextEdit::singleline(&mut draft.name).desired_width(260.0));
                ui.end_row();
                ui.label("Client");
                ui.vertical(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut draft.client)
                            .hint_text("Optional")
                            .desired_width(260.0),
                    );
                    let typed = draft.client.trim().to_lowercase();
                    let matches: Vec<_> = data
                        .clients
                        .iter()
                        .filter(|client| {
                            !typed.is_empty()
                                && client.name.to_lowercase().starts_with(&typed)
                                && client.name != draft.client
                        })
                        .take(4)
                        .collect();
                    if !matches.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            for client in matches {
                                if ui.small_button(&client.name).clicked() {
                                    draft.client = client.name.clone();
                                }
                            }
                        });
                    }
                });
                ui.end_row();
                ui.label("Colour");
                ui.horizontal_wrapped(|ui| {
                    for colour in PALETTE {
                        let selected = draft.colour == colour;
                        let (rect, response) =
                            ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::click());
                        ui.painter()
                            .circle_filled(rect.center(), 9.0, project_colour(colour));
                        if selected {
                            ui.painter().circle_stroke(
                                rect.center(),
                                11.0,
                                egui::Stroke::new(1.5, style::TEXT),
                            );
                        }
                        if response.on_hover_text(colour).clicked() {
                            draft.colour = colour.into();
                        }
                    }
                });
                ui.end_row();
            });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                actions.push(Action::SaveProject);
            }
            if ui.button("Cancel").clicked() {
                actions.push(Action::CancelProject);
            }
        });
    });
}
