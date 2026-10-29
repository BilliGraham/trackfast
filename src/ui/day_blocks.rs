use super::{Clock, style};
use crate::entries::{Snapshot, summary, week};
use crate::model::{Action, EntryDraft};
use eframe::egui::{self, RichText};

pub fn show(
    ui: &mut egui::Ui,
    data: &Snapshot,
    clock: &Clock,
    shown: jiff::civil::Date,
    actions: &mut Vec<Action>,
) {
    ui.weak("Daily work, without start and stop times.");
    egui::ScrollArea::both()
        .id_salt("day-blocks")
        .auto_shrink(false)
        .show(ui, |ui| {
            let width = ((ui.available_width() - 48.0) / 7.0).max(125.0);
            ui.horizontal_top(|ui| {
                for day in week::days(shown) {
                    ui.push_id(day, |ui| {
                        ui.vertical(|ui| {
                            ui.set_width(width);
                            ui.strong(day.strftime("%a %-d %b").to_string());
                            let entries: Vec<_> = data
                                .sessions
                                .iter()
                                .filter(|s| s.date(&clock.tz) == day)
                                .collect();
                            let total =
                                entries.iter().fold(jiff::SignedDuration::ZERO, |sum, s| {
                                    sum + s.effective(clock.now)
                                });
                            ui.weak(summary::clock(total));
                            if ui
                                .add_sized([width, 30.0], egui::Button::new("+ Add duration"))
                                .clicked()
                            {
                                let mut draft = EntryDraft::day_block(clock.now, &clock.tz);
                                draft.date = day.to_string();
                                draft.end_date = day.to_string();
                                actions.push(Action::DraftEntry(Box::new(draft)));
                            }
                            for entry in entries {
                                ui.push_id(entry.id, |ui| {
                                    let seconds = entry.effective(clock.now).as_secs();
                                    let height =
                                        (seconds as f32 / 3600.0 * 48.0).clamp(70.0, 300.0);
                                    let colour = super::session_colour(data, entry.project_id);
                                    let response = egui::Frame::new()
                                        .fill(style::SURFACE)
                                        .stroke(egui::Stroke::new(1.0, colour.gamma_multiply(0.6)))
                                        .corner_radius(4)
                                        .inner_margin(8)
                                        .show(ui, |ui| {
                                            ui.set_width(width - 18.0);
                                            ui.set_min_height(height);
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(super::description(entry))
                                                        .strong(),
                                                )
                                                .wrap(),
                                            );
                                            super::project_label(ui, data, entry.project_id);
                                            ui.label(summary::clock(entry.effective(clock.now)));
                                            super::sync_status_label(ui, data, entry.id);
                                            if entry.is_running() {
                                                ui.weak("Running");
                                            }
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut().button_padding =
                                                    egui::vec2(8.0, 4.0);
                                                ui.spacing_mut().item_spacing.x = 5.0;
                                                if ui
                                                    .add_enabled(
                                                        !entry.is_running(),
                                                        egui::Button::new("Edit"),
                                                    )
                                                    .clicked()
                                                {
                                                    actions.push(Action::EditEntry(Some(entry.id)));
                                                }
                                                if let Some(row) = data.rows.iter().find(|r| {
                                                    r.session
                                                        .as_ref()
                                                        .is_some_and(|s| s.id == entry.id)
                                                }) {
                                                    if ui.button("Pause").clicked() {
                                                        actions.push(Action::Pause(row.id));
                                                    }
                                                } else if ui
                                                    .add_enabled(
                                                        data.has_room(),
                                                        egui::Button::new("Start"),
                                                    )
                                                    .clicked()
                                                {
                                                    actions.push(Action::AddTask(
                                                        crate::entries::Task {
                                                            description: entry.description.clone(),
                                                            project_id: entry.project_id,
                                                        },
                                                    ));
                                                }
                                            });
                                        });
                                    response.response.on_hover_text(format!(
                                        "Recorded {}",
                                        summary::clock(entry.elapsed(clock.now))
                                    ));
                                });
                            }
                        });
                    });
                }
            });
        });
}
