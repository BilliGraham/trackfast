use super::{Clock, style};
use crate::model::{Action, EntryDraft};
use eframe::egui::{self, Rect, Sense, Stroke, StrokeKind};
use jiff::civil::Date;

pub fn show(ui: &mut egui::Ui, rect: Rect, day: Date, clock: &Clock, actions: &mut Vec<Action>) {
    let id = ui.id().with(("calendar-selection", day));
    let response = ui
        .interact(rect, id, Sense::click_and_drag())
        .on_hover_cursor(egui::CursorIcon::Crosshair);
    let minute = |y: f32| {
        (((y - rect.top()) / rect.height() * 1440.0 / 5.0).round() * 5.0).clamp(0.0, 1440.0) as u16
    };
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        ui.data_mut(|d| d.remove::<u16>(id));
        return;
    }
    if response.drag_started()
        && let Some(origin) = ui.input(|i| i.pointer.press_origin())
    {
        ui.data_mut(|d| d.insert_temp(id, minute(origin.y).min(1435)));
    }
    let anchor = ui.data(|d| d.get_temp::<u16>(id));
    if let Some(start) = anchor
        && let Some(pos) = response.interact_pointer_pos()
    {
        let end = minute(pos.y);
        let top = rect.top() + f32::from(start.min(end)) / 1440.0 * rect.height();
        let bottom = rect.top() + f32::from(start.max(end)) / 1440.0 * rect.height();
        let preview = Rect::from_min_max(
            egui::pos2(rect.left() + 2.0, top),
            egui::pos2(rect.right() - 2.0, bottom.max(top + 3.0)),
        );
        ui.painter()
            .rect_filled(preview, 3.0, style::ACCENT.gamma_multiply(0.2));
        ui.painter().rect_stroke(
            preview,
            3.0,
            Stroke::new(1.0, style::ACCENT),
            StrokeKind::Inside,
        );
        if response.drag_stopped() {
            ui.data_mut(|d| d.remove::<u16>(id));
            let end = if start == end {
                (start + 5).min(1440)
            } else {
                end
            };
            if let Some(form) = EntryDraft::calendar_range(day, start, end, &clock.tz) {
                actions.push(Action::DraftEntry(Box::new(form)));
            }
        }
    }
    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let start = minute(pos.y).min(1435);
        if let Some(form) =
            EntryDraft::calendar_range(day, start, (start + 30).min(1440), &clock.tz)
        {
            actions.push(Action::DraftEntry(Box::new(form)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui;
    fn frame(ctx: &egui::Context, events: Vec<egui::Event>) -> Vec<crate::model::Action> {
        let mut actions = Vec::new();
        let mut output = ctx.run_ui(
            egui::RawInput {
                events,
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(300.0, 1600.0),
                )),
                ..Default::default()
            },
            |ui| {
                show(
                    ui,
                    egui::Rect::from_min_max(egui::pos2(10.0, 10.0), egui::pos2(110.0, 1450.0)),
                    "2026-10-27".parse().unwrap(),
                    &crate::ui::Clock {
                        now: "2026-10-27T12:00:00Z".parse().unwrap(),
                        tz: jiff::tz::TimeZone::UTC,
                    },
                    &mut actions,
                );
            },
        );
        output.textures_delta.clear();
        actions
    }
    fn pointer(y: f32, pressed: bool) -> Vec<egui::Event> {
        let pos = egui::pos2(50.0, y);
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            },
        ]
    }
    #[test]
    fn drag_creates_one_selected_range_and_click_prepares_a_short_entry() {
        let ctx = egui::Context::default();
        frame(&ctx, vec![]);
        assert!(frame(&ctx, pointer(550.0, true)).is_empty());
        assert!(
            frame(
                &ctx,
                vec![egui::Event::PointerMoved(egui::pos2(50.0, 610.0))]
            )
            .is_empty()
        );
        let actions = frame(&ctx, pointer(610.0, false));
        let crate::model::Action::DraftEntry(form) = &actions[0] else {
            panic!("Expected a draft")
        };
        assert_eq!(form.start, "09:00:00");
        assert_eq!(form.stop, "10:00:00");
        assert_eq!(actions.len(), 1);
        frame(&ctx, pointer(670.0, true));
        let actions = frame(&ctx, pointer(670.0, false));
        let crate::model::Action::DraftEntry(form) = &actions[0] else {
            panic!("Expected a draft")
        };
        assert_eq!(form.start, "11:00:00");
        assert_eq!(form.stop, "11:30:00");
    }
    #[test]
    fn escape_cancels_a_drag_without_creating_an_entry() {
        let ctx = egui::Context::default();
        frame(&ctx, vec![]);
        frame(&ctx, pointer(550.0, true));
        frame(
            &ctx,
            vec![egui::Event::PointerMoved(egui::pos2(50.0, 610.0))],
        );
        frame(
            &ctx,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        assert!(frame(&ctx, pointer(610.0, false)).is_empty());
    }
}
