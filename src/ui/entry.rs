use crate::entries::summary;
use crate::model::{Action, Drafts, State};
use eframe::egui;

pub fn show(ctx: &egui::Context, state: &State, drafts: &mut Drafts, actions: &mut Vec<Action>) {
    if let Some(form) = &mut drafts.entry {
        egui::Window::new(if form.id == 0 {
            "Add entry"
        } else {
            "Edit entry"
        })
        .collapsible(false)
        .auto_sized()
        .default_width(460.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            super::style::apply(ui);
            ui.add_enabled_ui(!state.saving, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut form.timed, false, "Duration only");
                    ui.selectable_value(&mut form.timed, true, "Start and stop");
                });
                ui.add_space(10.0);
                ui.label("Description");
                let description = ui.add(
                    egui::TextEdit::singleline(&mut form.description).desired_width(f32::INFINITY),
                );
                if form.focus_description {
                    description.request_focus();
                    form.focus_description = false;
                }
                ui.label("Project");
                egui::ComboBox::from_id_salt("entry-project")
                    .selected_text(
                        state
                            .data
                            .as_ref()
                            .and_then(|d| form.project_id.and_then(|id| d.project(id)))
                            .map(|p| p.name.as_str())
                            .unwrap_or("No project"),
                    )
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut form.project_id, None, "No project");
                        if let Some(data) = &state.data {
                            for project in data.projects.iter().filter(|p| !p.archived) {
                                ui.selectable_value(
                                    &mut form.project_id,
                                    Some(project.id),
                                    &project.name,
                                );
                            }
                        }
                    });
                if form.timed {
                    date_time(ui, "Start", &mut form.date, &mut form.start);
                    date_time(ui, "Stop", &mut form.end_date, &mut form.stop);
                    field(ui, "Time zone", &mut form.time_zone);
                } else {
                    field(ui, "Date", &mut form.date);
                    field(ui, "Duration (H:MM:SS)", &mut form.duration);
                }
                ui.add_space(8.0);
                let valid = match form.session() {
                    Ok(mut entry) => {
                        entry.details.round_to_five = form.round_to_five.unwrap_or_else(|| {
                            state.data.as_ref().is_none_or(|d| d.settings.round_to_five)
                        });
                        ui.label(format!(
                            "Recorded {}  ·  Total {}",
                            summary::clock(entry.elapsed(entry.start)),
                            summary::clock(entry.effective(entry.start))
                        ));
                        if entry.effective(entry.start).is_zero() {
                            ui.weak("This entry stays visible with a zero total.");
                        }
                        true
                    }
                    Err(error) => {
                        ui.weak(error);
                        false
                    }
                };
                if let Some(problem) = &state.problem {
                    ui.colored_label(ui.visuals().warn_fg_color, problem);
                }
                ui.separator();
                if form.id == 0 {
                    let can_start = !form.description.trim().is_empty()
                        && state.data.as_ref().is_some_and(|d| d.has_room());
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(can_start, egui::Button::new("Start now"))
                            .on_disabled_hover_text(
                                "Enter a description. The widget must have room for another task.",
                            )
                            .clicked()
                        {
                            actions.push(Action::StartEntry);
                        }
                        ui.weak("Uses the current time, not the selected range.");
                    });
                }
                ui.horizontal(|ui| {
                    let shortcut = ui.input_mut(|input| {
                        input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)
                    });
                    if ui
                        .add_enabled(valid, egui::Button::new("Save entry"))
                        .clicked()
                        || (valid && shortcut)
                    {
                        actions.push(Action::SaveEntry);
                    }
                    if ui.button("Cancel").clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        actions.push(Action::CancelEntry);
                    }
                    ui.weak("Ctrl / Cmd + Enter to save");
                });
            });
            if state.saving {
                ui.label("Saving…");
            }
        });
    }
    if let Some(id) = drafts.delete_entry {
        egui::Window::new("Delete entry?")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(
                    "This removes the entry from this computer. This action cannot be undone.",
                );
                if let Some(problem) = &state.problem {
                    ui.colored_label(ui.visuals().warn_fg_color, problem);
                }
                ui.add_enabled_ui(!state.saving, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("Delete entry").clicked() {
                            actions.push(Action::DeleteEntry(id));
                        }
                        if ui.button("Cancel").clicked() {
                            actions.push(Action::CancelEntry);
                        }
                    });
                });
            });
    }
}
fn field(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.horizontal(|ui| {
        ui.add_sized([125.0, 32.0], egui::Label::new(label));
        ui.add(egui::TextEdit::singleline(value).desired_width(210.0));
    });
}

fn date_time(ui: &mut egui::Ui, label: &str, date: &mut String, time: &mut String) {
    ui.horizontal(|ui| {
        ui.add_sized([80.0, 32.0], egui::Label::new(label));
        ui.add(egui::TextEdit::singleline(date).desired_width(105.0));
        ui.add(egui::TextEdit::singleline(time).desired_width(95.0));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timed_entry_buttons_fit_without_scrolling() {
        for size in [egui::vec2(1150.0, 720.0), egui::vec2(760.0, 480.0)] {
            let ctx = egui::Context::default();
            let now = "2026-10-26T09:00:00Z".parse().unwrap();
            let mut form = crate::model::EntryDraft::new(now, &jiff::tz::TimeZone::UTC);
            form.timed = true;
            form.description = "Design".into();
            let mut drafts = Drafts {
                entry: Some(form),
                ..Default::default()
            };
            let state = State {
                data: Some(Default::default()),
                ..Default::default()
            };
            for frame in 0..4 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                        ..Default::default()
                    },
                    |_| show(&ctx, &state, &mut drafts, &mut Vec::new()),
                );
                output.textures_delta.clear();
                if frame == 3 {
                    for label in ["Save entry", "Cancel"] {
                        let visible = output.shapes.iter().any(|shape| {
                            if let egui::epaint::Shape::Text(text) = &shape.shape {
                                text.galley.text() == label
                                    && shape.clip_rect.contains_rect(
                                        text.galley.rect.translate(text.pos.to_vec2()),
                                    )
                            } else {
                                false
                            }
                        });
                        assert!(visible, "{label} must fit at {size:?}");
                    }
                }
            }
        }
    }
    #[test]
    fn keyboard_save_keeps_the_draft_until_the_worker_confirms() {
        let ctx = egui::Context::default();
        let now = "2026-10-26T09:00:00Z".parse().unwrap();
        let mut form = crate::model::EntryDraft::new(now, &jiff::tz::TimeZone::UTC);
        form.description = "Design".into();
        form.duration = "3:00:01".into();
        let mut drafts = Drafts {
            entry: Some(form),
            ..Default::default()
        };
        let state = State {
            data: Some(Default::default()),
            ..Default::default()
        };
        let mut actions = Vec::new();
        for frame in 0..3 {
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(760.0, 480.0),
                )),
                ..Default::default()
            };
            if frame == 2 {
                input.events.push(egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::COMMAND,
                });
            }
            let mut output = ctx.run_ui(input, |_| show(&ctx, &state, &mut drafts, &mut actions));
            output.textures_delta.clear();
        }
        assert_eq!(
            actions
                .iter()
                .filter(|a| matches!(a, Action::SaveEntry))
                .count(),
            1
        );
        assert_eq!(drafts.entry.as_ref().unwrap().duration, "3:00:01");
    }
}
