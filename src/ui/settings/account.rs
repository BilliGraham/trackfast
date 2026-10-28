use crate::{
    account::{Status, View},
    model::{Action, Drafts},
    ui::style,
};
use eframe::egui;

fn field_id() -> egui::Id {
    egui::Id::new("account-desktop-secret")
}
pub fn clear_secret_input(ctx: &egui::Context) {
    ctx.data_mut(|data| data.remove::<egui::text_edit::TextEditState>(field_id()));
}
pub fn show(ui: &mut egui::Ui, account: &View, drafts: &mut Drafts, actions: &mut Vec<Action>) {
    style::card().inner_margin(16).show(ui, |ui| {
        ui.set_width(ui.available_width().min(460.0));
        ui.strong("Trackfast account");
        ui.add_space(8.0);
        ui.label(match account.status {
            Status::Unconfigured => "Account sign-in is not configured for this local pilot.",
            Status::Checking => "Checking the saved session...",
            Status::SignedOut => "Sign in to the local Trackfast API.",
            Status::SigningIn => "Complete sign-in in your browser.",
            Status::Connected => "Connected to Trackfast.",
            Status::SigningOut => "Signing out...",
            Status::Failed => "The account needs attention.",
        });
        if account.status != Status::Unconfigured {
            ui.weak("Time tracking stays available offline. Control entry delivery in sync setup below.");
            ui.add_space(8.0);
            if matches!(account.status, Status::SignedOut | Status::Failed) {
                ui.label("Google desktop client secret (if supplied)");
                let response = ui.add(
                    egui::TextEdit::singleline(&mut *drafts.account_secret)
                        .id(field_id())
                        .password(true)
                        .desired_width(ui.available_width()),
                );
                if let Some(mut state) = egui::text_edit::TextEditState::load(ui.ctx(), response.id)
                {
                    // Password masking does not remove the secret from undo history.
                    state.clear_undoer();
                    state.store(ui.ctx(), response.id);
                }
                ui.weak("Used for this sign-in only. It is not saved.");
            }
            ui.horizontal_wrapped(|ui| {
                if account.status.busy() {
                    ui.spinner();
                    if account.status == Status::SigningIn && ui.button("Cancel sign-in").clicked()
                    {
                        actions.push(Action::CancelSignIn);
                    }
                } else {
                    if matches!(account.status, Status::SignedOut | Status::Failed)
                        && ui.button("Sign in with Google").clicked()
                    {
                        actions.push(Action::SignIn);
                    }
                    if matches!(account.status, Status::Connected | Status::Failed)
                        && ui.button("Check connection").clicked()
                    {
                        actions.push(Action::RestoreAccount);
                    }
                    if ui
                        .button(if account.status == Status::Failed {
                            "Retry sign-out"
                        } else {
                            "Sign out"
                        })
                        .clicked()
                    {
                        actions.push(Action::SignOut);
                    }
                }
            });
        }
        if let Some(problem) = &account.problem {
            ui.colored_label(ui.visuals().warn_fg_color, problem);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::{Status, View};

    #[test]
    fn submitted_secret_cannot_return_through_undo() {
        let ctx = egui::Context::default();
        let mut drafts = Drafts::default();
        let draw = |drafts: &mut Drafts, time, events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ui| {
                    show(
                        ui,
                        &View {
                            status: Status::SignedOut,
                            problem: None,
                        },
                        drafts,
                        &mut Vec::new(),
                    );
                },
            );
            output.textures_delta.clear();
        };
        draw(&mut drafts, 0.0, vec![]);
        ctx.memory_mut(|m| m.request_focus(field_id()));
        draw(&mut drafts, 0.1, vec![]);
        draw(
            &mut drafts,
            0.2,
            vec![egui::Event::Paste("synthetic-desktop-secret".into())],
        );
        assert_eq!(drafts.account_secret.as_str(), "synthetic-desktop-secret");
        draw(&mut drafts, 1.5, vec![]);
        drop(std::mem::take(&mut drafts.account_secret));
        clear_secret_input(&ctx);
        draw(&mut drafts, 2.0, vec![]);
        ctx.memory_mut(|m| m.request_focus(field_id()));
        draw(&mut drafts, 2.1, vec![]);
        draw(
            &mut drafts,
            2.2,
            vec![egui::Event::Key {
                key: egui::Key::Z,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers {
                    ctrl: true,
                    command: true,
                    ..Default::default()
                },
            }],
        );
        assert!(drafts.account_secret.is_empty());
    }

    #[test]
    fn account_buttons_fit_and_signin_click_returns_one_action() {
        for width in [340.0, 600.0] {
            let ctx = egui::Context::default();
            let state = View {
                status: Status::SignedOut,
                problem: None,
            };
            let mut drafts = Drafts::default();
            let mut actions = Vec::new();
            let mut point = None;
            for frame in 0..4 {
                let events = if frame >= 2 {
                    vec![
                        egui::Event::PointerMoved(point.unwrap()),
                        egui::Event::PointerButton {
                            pos: point.unwrap(),
                            button: egui::PointerButton::Primary,
                            pressed: frame == 2,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ]
                } else {
                    vec![]
                };
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 480.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| show(ui, &state, &mut drafts, &mut actions),
                );
                output.textures_delta.clear();
                for shape in &output.shapes {
                    if let egui::Shape::Text(text) = &shape.shape
                        && text.galley.text() == "Sign in with Google"
                    {
                        let rect = text.galley.rect.translate(text.pos.to_vec2());
                        assert!(shape.clip_rect.contains_rect(rect));
                        point = Some(rect.center());
                    }
                }
            }
            assert_eq!(
                actions
                    .iter()
                    .filter(|a| matches!(a, Action::SignIn))
                    .count(),
                1
            );
        }
    }
}
