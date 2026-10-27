use eframe::egui::{self, RichText};

use super::style;
use crate::model::{Action, Drafts, ImportState, State};

const TOKEN_PAGE: &str = "https://track.toggl.com/profile";

fn token_field_id() -> egui::Id {
    egui::Id::new("toggl-import-token")
}

pub fn clear_token_input(ctx: &egui::Context) {
    ctx.data_mut(|data| data.remove::<egui::text_edit::TextEditState>(token_field_id()));
}

pub fn show(ui: &mut egui::Ui, state: &State, drafts: &mut Drafts, actions: &mut Vec<Action>) {
    egui::ScrollArea::vertical().show(ui, |ui| contents(ui, state, drafts, actions));
}
fn contents(ui: &mut egui::Ui, state: &State, drafts: &mut Drafts, actions: &mut Vec<Action>) {
    ui.heading("Settings");
    ui.add_space(16.0);
    if let Some(data) = &state.data {
        let settings = drafts.settings.get_or_insert_with(|| data.settings.clone());
        style::card().inner_margin(16).show(ui, |ui| {
            ui.set_width(ui.available_width().min(460.0));
            ui.strong("Time and weekly target");
            ui.add_space(8.0);
            ui.add_enabled_ui(!state.saving, |ui| {
                ui.checkbox(
                    &mut settings.round_to_five,
                    "Round new entries to the nearest 5 minutes",
                );
                ui.weak("Off keeps exact seconds. Existing entries keep their rounding rule.");
                ui.horizontal(|ui| {
                    ui.label("Weekly target (hours)");
                    ui.add(
                        egui::DragValue::new(&mut settings.weekly_target_hours)
                            .range(0.1..=168.0)
                            .speed(0.5),
                    );
                });
                ui.weak("The weekly progress bar uses this target.");
                ui.add_space(8.0);
                if ui
                    .add_enabled(
                        *settings != data.settings,
                        egui::Button::new("Save settings"),
                    )
                    .clicked()
                {
                    actions.push(Action::SaveSettings);
                }
            });
            if state.saving {
                ui.label("Saving...");
            }
            if let Some(problem) = &state.problem {
                ui.colored_label(ui.visuals().warn_fg_color, problem);
            }
            if let Some(notice) = &state.notice {
                ui.weak(notice);
            }
        });
        ui.add_space(16.0);
    }

    style::card().inner_margin(16).show(ui, |ui| {
        ui.set_width(ui.available_width().min(460.0));
        ui.label(RichText::new("Import from Toggl").size(15.0).strong());
        ui.label("Copy your Toggl projects, clients, and completed time entries into Trackfast.");
        ui.label("Trackfast only reads from Toggl. It uses the token for this import and does not keep it.");
        ui.label("You can run the import again. It skips the entries that it already copied.");
        ui.hyperlink_to("Open your Toggl profile to copy the API token", TOKEN_PAGE);
        ui.add_space(8.0);
        let running = matches!(state.import, ImportState::Running(_));
        let field = ui.add_enabled(
            !running,
            egui::TextEdit::singleline(&mut drafts.import_token)
                .id(token_field_id())
                .password(true)
                .hint_text("API token")
                .desired_width(340.0),
        );
        if let Some(mut edit_state) = egui::text_edit::TextEditState::load(ui.ctx(), field.id) {
            // Password masking does not remove the plain token from egui undo history.
            edit_state.clear_undoer();
            edit_state.store(ui.ctx(), field.id);
        }
        ui.horizontal(|ui| {
            ui.label("Start date");
            ui.add_enabled(
                !running,
                egui::TextEdit::singleline(&mut drafts.import_from)
                    .hint_text("YYYY-MM-DD")
                    .desired_width(110.0),
            );
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!running, egui::Button::new("Import"))
                .clicked()
            {
                actions.push(Action::StartImport);
            }
            match &state.import {
                ImportState::Idle => {}
                ImportState::Running(step) => {
                    ui.spinner();
                    ui.label(step);
                }
                ImportState::Done(result) => {
                    ui.label(result);
                }
                ImportState::Failed(problem) => {
                    ui.colored_label(ui.visuals().warn_fg_color, problem);
                }
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draw(ctx: &egui::Context, drafts: &mut Drafts, time: f64, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(760.0, 480.0),
            )),
            time: Some(time),
            events,
            ..Default::default()
        };
        let state = State::default();
        let mut output = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                show(ui, &state, drafts, &mut Vec::new());
            });
        });
        output.textures_delta.clear();
    }

    #[test]
    fn token_undo_cannot_restore_a_submitted_token() {
        let ctx = egui::Context::default();
        let mut drafts = Drafts::default();
        draw(&ctx, &mut drafts, 0.0, vec![]);
        ctx.memory_mut(|memory| memory.request_focus(token_field_id()));
        draw(&ctx, &mut drafts, 0.1, vec![]);
        draw(
            &ctx,
            &mut drafts,
            0.2,
            vec![egui::Event::Paste("synthetic-review-token".into())],
        );
        assert_eq!(drafts.import_token, "synthetic-review-token");
        draw(&ctx, &mut drafts, 1.5, vec![]);
        drafts.import_token.clear();
        clear_token_input(&ctx);
        draw(&ctx, &mut drafts, 2.0, vec![]);
        ctx.memory_mut(|memory| memory.request_focus(token_field_id()));
        draw(&ctx, &mut drafts, 2.1, vec![]);
        let command = egui::Modifiers {
            ctrl: true,
            command: true,
            ..Default::default()
        };
        draw(
            &ctx,
            &mut drafts,
            2.2,
            vec![egui::Event::Key {
                key: egui::Key::Z,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: command,
            }],
        );
        assert!(drafts.import_token.is_empty());
    }
}
