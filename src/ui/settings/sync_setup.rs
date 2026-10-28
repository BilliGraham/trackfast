use crate::{
    account::{SetupRequest, Status},
    model::{Action, Drafts, State},
    ui::style,
};
use eframe::egui;

pub fn show(ui: &mut egui::Ui, state: &State, drafts: &mut Drafts, actions: &mut Vec<Action>) {
    let Some(data) = &state.data else {
        return;
    };
    style::card().inner_margin(16).show(ui, |ui| {
        ui.set_width(ui.available_width().min(460.0));
        ui.strong("Time-entry sync setup");
        ui.add_space(8.0);
        ui.weak("Entry delivery is not enabled yet. This setup saves the account, the starting point, and project mappings.");
        let connected = state.account.status == Status::Connected;
        if !connected { ui.label("Sign in before you prepare sync or select API projects."); }
        if let Some(enrollment) = &data.enrollment {
            ui.label("The starting point is saved. Existing entries remain local.");
            ui.weak("Timers that were already running at setup remain local. Imported history remains local.");
            ui.label(format!("{} new completed entries saved locally since setup.", enrollment.new_entries));
            ui.weak("Signing out or restarting does not reset this starting point.");
            ui.add_space(8.0);
            ui.add_enabled_ui(connected && !state.setup_busy, |ui| {
                if ui.button("Refresh API projects").clicked() { actions.push(Action::SyncSetup(SetupRequest::Projects)); }
            });
            if let Some((scope, projects)) = &state.sync_projects {
                if scope != &enrollment.scope {
                    ui.colored_label(ui.visuals().warn_fg_color, "This setup belongs to another account or API. Sign in with the original configuration.");
                    return;
                }
                for (local, remote) in &enrollment.mappings {
                    let local_name = data.project(*local).map(|p| p.name.as_str()).unwrap_or("Unavailable local project");
                    let remote_name = projects.iter().find(|p| p.project_id==*remote).map(|p| p.name.clone()).unwrap_or_else(|| format!("Unavailable API project ({remote})"));
                    ui.label(format!("{local_name} → {remote_name}"));
                }
                ui.add_space(8.0);
                ui.add_enabled_ui(connected && !state.setup_busy, |ui| {
                    ui.label("Local project");
                    egui::ComboBox::from_id_salt("sync-local-project").width(ui.available_width().min(300.0)).selected_text(
                        drafts.sync_local_project.and_then(|id| data.project(id)).map(|p| p.name.as_str()).unwrap_or("Select a local project")
                    ).show_ui(ui, |ui| {
                        for project in data.projects.iter().filter(|p| !p.archived && !enrollment.mappings.iter().any(|(id, _)| *id == p.id)) {
                            ui.selectable_value(&mut drafts.sync_local_project, Some(project.id), &project.name);
                        }
                    });
                    ui.label("API project");
                    egui::ComboBox::from_id_salt("sync-remote-project").width(ui.available_width().min(300.0)).selected_text(
                        projects.iter().find(|p| Some(p.project_id)==drafts.sync_project).map(|p| p.name.as_str()).unwrap_or("Select an API project")
                    ).show_ui(ui, |ui| {
                        for project in projects.iter().filter(|p| !p.archived) {
                            ui.selectable_value(&mut drafts.sync_project, Some(project.project_id), format!("{} ({})", project.name, project.project_id));
                        }
                    });
                    let selection = drafts.sync_local_project.zip(drafts.sync_project).filter(|(local, remote)| {
                        data.project(*local).is_some_and(|p| !p.archived)
                            && !enrollment.mappings.iter().any(|(id, _)| id == local)
                            && projects.iter().any(|p| p.project_id == *remote && !p.archived)
                    });
                    ui.weak("Check both projects before you save. Reassignment is not supported yet.");
                    if ui.add_enabled(selection.is_some(), egui::Button::new("Save mapping")).clicked()
                        && let Some((local, remote)) = selection {
                        actions.push(Action::SyncSetup(SetupRequest::Map(local, remote)));
                    }
                    if projects.iter().all(|p| p.archived) { ui.label("No active API projects are available for this account."); }
                });
            } else {
                ui.weak(format!("{} project mappings saved. Refresh to review API project names.", enrollment.mappings.len()));
            }
            ui.weak("Projects without a mapping will wait for setup. Entries without a project will remain unassigned.");
        } else {
            ui.label("Keep all current entries on this computer. Prepare only work created after you save setup.");
            ui.weak("This also excludes timers that are already running.");
            if connected {
                ui.add_enabled_ui(!state.setup_busy, |ui| {
                    if ui.button("Prepare new work for sync").clicked() { actions.push(Action::SyncSetup(SetupRequest::Enroll)); }
                });
            }
        }
        if state.setup_busy { ui.spinner(); ui.label("Checking and saving sync setup..."); }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        account::{SetupRequest, Status},
        entries::Snapshot,
        model::{Action, Drafts, State},
    };
    use eframe::egui;

    #[test]
    fn setup_button_requires_connection_and_returns_one_explicit_action() {
        for width in [340.0, 600.0] {
            let ctx = egui::Context::default();
            let mut state = State {
                data: Some(Snapshot::default()),
                ..Default::default()
            };
            let mut drafts = Drafts::default();
            let mut actions = Vec::new();
            let mut output = ctx.run_ui(Default::default(), |ui| {
                show(ui, &state, &mut drafts, &mut actions)
            });
            output.textures_delta.clear();
            assert!(!output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text()=="Prepare new work for sync")));
            state.account.status = Status::Connected;
            let mut point = None;
            for frame in 0..4 {
                let events = if frame < 2 {
                    vec![]
                } else {
                    vec![
                        egui::Event::PointerMoved(point.unwrap()),
                        egui::Event::PointerButton {
                            pos: point.unwrap(),
                            button: egui::PointerButton::Primary,
                            pressed: frame == 2,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ]
                };
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 800.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| show(ui, &state, &mut drafts, &mut actions),
                );
                output.textures_delta.clear();
                for shape in output.shapes {
                    if let egui::Shape::Text(text) = shape.shape {
                        let rect = text.galley.rect.translate(text.pos.to_vec2());
                        assert!(
                            shape.clip_rect.contains_rect(rect),
                            "{}",
                            text.galley.text()
                        );
                        if text.galley.text() == "Prepare new work for sync" {
                            point = Some(rect.center());
                        }
                    }
                }
            }
            assert_eq!(
                actions
                    .iter()
                    .filter(|a| matches!(a, Action::SyncSetup(SetupRequest::Enroll)))
                    .count(),
                1
            );
        }
    }

    #[test]
    fn mapping_controls_fit_both_themes_and_reject_another_account() {
        use uuid::Uuid;
        let scope = trackfast_sync::Scope {
            server: "http://127.0.0.1:8788".into(),
            tenant_id: Uuid::new_v4(),
            person_id: Uuid::new_v4(),
        };
        for (width, dark) in [(340.0, true), (600.0, true), (340.0, false), (600.0, false)] {
            let ctx = egui::Context::default();
            ctx.set_visuals(if dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            });
            let remote = Uuid::new_v4();
            let mut state = State {
                data: Some(Snapshot {
                    enrollment: Some(crate::db::enrollment::Enrollment {
                        scope: scope.clone(),
                        boundary: 3,
                        new_entries: 2,
                        mappings: vec![],
                    }),
                    projects: vec![crate::entries::Project {
                        id: 1,
                        name: "Local project".into(),
                        colour: "#abcdef".into(),
                        client_id: None,
                        archived: false,
                    }],
                    ..Default::default()
                }),
                sync_projects: Some((
                    scope.clone(),
                    vec![trackfast_sync::transport::Project {
                        project_id: remote,
                        name: "API project".into(),
                        colour: "#abcdef".into(),
                        archived: false,
                    }],
                )),
                ..Default::default()
            };
            state.account.status = Status::Connected;
            let mut drafts = Drafts {
                sync_local_project: Some(1),
                sync_project: Some(remote),
                ..Default::default()
            };
            let mut actions = vec![];
            let mut point = None;
            for frame in 0..4 {
                let events = if frame < 2 {
                    vec![]
                } else {
                    vec![
                        egui::Event::PointerMoved(point.unwrap()),
                        egui::Event::PointerButton {
                            pos: point.unwrap(),
                            button: egui::PointerButton::Primary,
                            pressed: frame == 2,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ]
                };
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 1000.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| show(ui, &state, &mut drafts, &mut actions),
                );
                output.textures_delta.clear();
                for shape in output.shapes {
                    if let egui::Shape::Text(text) = shape.shape {
                        let rect = text.galley.rect.translate(text.pos.to_vec2());
                        assert!(
                            shape.clip_rect.contains_rect(rect),
                            "{}",
                            text.galley.text()
                        );
                        if text.galley.text() == "Save mapping" {
                            point = Some(rect.center());
                        }
                    }
                }
            }
            assert_eq!(
                actions
                    .iter()
                    .filter(
                        |a| matches!(a, Action::SyncSetup(SetupRequest::Map(1, id)) if *id==remote)
                    )
                    .count(),
                1
            );
            state.sync_projects.as_mut().unwrap().0.person_id = Uuid::new_v4();
            let mut output = ctx.run_ui(Default::default(), |ui| {
                show(ui, &state, &mut drafts, &mut actions)
            });
            output.textures_delta.clear();
            assert!(!output.shapes.iter().any(
                |s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text()=="Save mapping")
            ));
        }
    }
}
