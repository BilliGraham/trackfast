use crate::{
    account::Status,
    model::{Action, State},
    ui::style,
};
use eframe::egui;
use trackfast_sync::{Entry, native_resolution::Choice};

pub fn show(ui: &mut egui::Ui, state: &State, actions: &mut Vec<Action>) {
    let Some(enrollment) = state.data.as_ref().and_then(|d| d.enrollment.as_ref()) else {
        return;
    };
    ui.add_space(16.0);
    style::card().inner_margin(16).show(ui, |ui| {
        ui.set_width(ui.available_width().min(460.0));
        ui.strong("Conflict review");
        ui.weak("Review stops delivery. A choice resumes delivery and preserves later local work.");
        let enabled = state.account.status==Status::Connected && !state.conflict_busy && !state.setup_busy;
        let more = state.conflict_review.as_ref().is_some_and(|(_,_,more)|*more);
        if ui.add_enabled(enabled, egui::Button::new(if more {"Load more API changes"} else {"Review conflicts"})).clicked() {
            actions.push(Action::ReviewConflicts);
        }
        if state.conflict_busy {ui.spinner(); ui.label("Loading or saving the review...");}
        if state.account.status!=Status::Connected {return;}
        let Some((scope, conflicts, more)) = &state.conflict_review else {return;};
        if scope!=&enrollment.scope {ui.label("This review belongs to another account.");return;}
        if conflicts.is_empty() {ui.label("No pending conflicts were found.");}
        if *more {ui.weak("Load the remaining changes before choosing a version.");}
        for conflict in conflicts {
            ui.push_id(conflict.operation_id, |ui| {
                ui.separator();
                ui.strong("Submitted local version");
                if conflict.action=="delete" {ui.label("Delete this entry");}
                entry(ui,&conflict.local);
                ui.strong("API version");
                let Some(remote) = &conflict.remote else {ui.label("The API version is not available. Refresh the review.");return;};
                ui.weak(format!("Version {}",remote.version));
                if let Some(remote) = &remote.entry {entry(ui,remote);} else {ui.label("Deleted in the API");}
                ui.weak("Use local retries the submitted values. Later local edits stay queued.");
                ui.weak("Use API accepts its values. Later local edits are preserved as a separate entry. Later deletions stay queued.");
                ui.weak("Keep a local copy accepts the API version and creates a new entry from local work. The copy will be sent separately.");
                ui.add_enabled_ui(enabled && !*more, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        let can_local = remote.entry.as_ref().is_some_and(|e|e.recorded_settings==conflict.local.recorded_settings);
                        if ui.add_enabled(can_local,egui::Button::new("Use local version")).clicked() {actions.push(Action::ResolveConflict(conflict.operation_id,remote.version,Choice::Local));}
                        if ui.button("Use API version").clicked() {actions.push(Action::ResolveConflict(conflict.operation_id,remote.version,Choice::Remote));}
                        if ui.add_enabled(conflict.action!="delete",egui::Button::new("Keep a local copy")).clicked() {actions.push(Action::ResolveConflict(conflict.operation_id,remote.version,Choice::Copy));}
                    });
                });
            });
        }
    });
}

fn entry(ui: &mut egui::Ui, entry: &Entry) {
    ui.add(egui::Label::new(&entry.description).wrap());
    ui.label(format!(
        "{} · {} recorded seconds",
        entry.entry_date, entry.original_seconds
    ));
    if let Some(timing) = &entry.timing {
        ui.add(
            egui::Label::new(format!(
                "{} to {} ({})",
                timing.started_at, timing.stopped_at, timing.time_zone
            ))
            .wrap(),
        );
    } else {
        ui.weak("Duration only");
    }
    ui.add(
        egui::Label::new(
            entry
                .project_id
                .map(|id| format!("Project: {id}"))
                .unwrap_or_else(|| "No project".into()),
        )
        .wrap(),
    );
    ui.weak(
        if entry
            .recorded_settings
            .as_ref()
            .and_then(|s| s.rounding_interval_minutes)
            == Some(5)
        {
            "Rounding: nearest 5 minutes"
        } else {
            "Rounding: off"
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        account::Status,
        model::{Action, State},
    };
    use eframe::egui;
    use trackfast_sync::{Conflict, Entry, RecordedSettings, Scope, transport::RemoteChange};
    use uuid::Uuid;
    #[test]
    fn review_shows_both_versions_and_submits_only_the_selected_choice() {
        for width in [340.0, 600.0] {
            for dark in [true, false] {
                let ctx = egui::Context::default();
                ctx.set_visuals(if dark {
                    egui::Visuals::dark()
                } else {
                    egui::Visuals::light()
                });
                let scope = Scope {
                    server: "http://127.0.0.1:8788".into(),
                    tenant_id: Uuid::new_v4(),
                    person_id: Uuid::new_v4(),
                };
                let local = Entry {
                    entry_id: Uuid::new_v4(),
                    project_id: None,
                    timing: None,
                    entry_date: "2026-10-28".parse().unwrap(),
                    description: "Local review text".into(),
                    original_seconds: 780,
                    recorded_settings: Some(RecordedSettings {
                        rounding_interval_minutes: Some(5),
                    }),
                };
                let operation = Uuid::new_v4();
                let mut state = State::default();
                state.account.status = Status::Connected;
                state.data = Some(crate::entries::Snapshot {
                    enrollment: Some(crate::db::enrollment::Enrollment {
                        scope: scope.clone(),
                        boundary: 0,
                        new_entries: 1,
                        mappings: vec![],
                    }),
                    ..Default::default()
                });
                state.conflict_review = Some((
                    scope,
                    vec![Conflict {
                        operation_id: operation,
                        action: "replace".into(),
                        remote: Some(RemoteChange {
                            entry_id: local.entry_id,
                            version: 2,
                            entry: Some(Entry {
                                description: "API review text".into(),
                                ..local.clone()
                            }),
                        }),
                        local,
                    }],
                    false,
                ));
                let mut point = None;
                let mut actions = Vec::new();
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
                                modifiers: Default::default(),
                            },
                        ]
                    };
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, 1400.0),
                            )),
                            events,
                            ..Default::default()
                        },
                        |ui| show(ui, &state, &mut actions),
                    );
                    let mut seen = [false, false];
                    for shape in &output.shapes {
                        if let egui::Shape::Text(text) = &shape.shape {
                            let rect = text.galley.rect.translate(text.pos.to_vec2());
                            assert!(
                                shape.clip_rect.contains_rect(rect),
                                "{}",
                                text.galley.text()
                            );
                            seen[0] |= text.galley.text() == "Local review text";
                            seen[1] |= text.galley.text() == "API review text";
                            if text.galley.text() == "Use API version" {
                                point = Some(rect.center());
                            }
                        }
                    }
                    assert_eq!(seen, [true, true]);
                    output.textures_delta.clear();
                }
                assert_eq!(actions.iter().filter(|a|matches!(a,Action::ResolveConflict(id,2,trackfast_sync::native_resolution::Choice::Remote) if *id==operation)).count(),1);
            }
        }
    }
}
