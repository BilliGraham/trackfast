use eframe::egui::{self, RichText};

use crate::model::widget::{MAX_TASKS, PinAvailability, PinnedTask, Widget};
use crate::model::{Action, State};
use crate::toggl::Snapshot;

use super::{Clock, description, dot, project_colour, right_aligned_row, style};

pub fn height(widget: &Widget) -> f32 {
    match widget.pins().len() {
        0 | 1 => 160.0,
        2 => 232.0,
        _ => 304.0,
    }
}

pub fn show(ui: &mut egui::Ui, state: &State, clock: &Clock) -> Vec<Action> {
    style::apply(ui);
    ui.spacing_mut().button_padding = egui::vec2(10.0, 4.0);
    ui.spacing_mut().interact_size.y = 26.0;
    ui.spacing_mut().item_spacing.y = 6.0;
    let mut actions = Vec::new();
    let show_menu = ui
        .ctx()
        .pointer_hover_pos()
        .is_some_and(|pos| ui.max_rect().contains(pos))
        || egui::Popup::is_any_open(ui.ctx())
        || ui.ctx().memory(|memory| memory.focused().is_some());
    ui.label(RichText::new("Pinned tasks").size(16.0).strong());
    ui.add_space(4.0);
    egui::ScrollArea::vertical()
        .id_salt("widget-rows")
        .auto_shrink([false, true])
        .max_height((ui.available_height() - 36.0).max(32.0))
        .show(ui, |ui| {
            if state.widget.pins().is_empty() {
                style::card().inner_margin(8).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label("No task selected");
                    ui.weak("Use Add task to choose an entry.");
                });
            }
            for pin in state.widget.pins() {
                ui.push_id(pin.entry.id, |ui| row(ui, pin, clock, &mut actions));
            }
        });
    ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
        ui.scope(|ui| {
            if !show_menu {
                ui.set_opacity(0.0);
            }
            super::actions::menu(ui, "Widget actions", |ui| {
                let can_add = state.widget.pins().len() < MAX_TASKS;
                let add = ui.add_enabled_ui(can_add && state.toggl.snapshot.is_some(), |ui| {
                    ui.menu_button("Add task", |ui| {
                        if let Some(snapshot) = &state.toggl.snapshot {
                            picker(ui, &state.widget, snapshot, clock, &mut actions);
                        }
                    })
                });
                if !can_add {
                    add.inner.response.on_disabled_hover_text(
                        "The widget has three tasks. Remove one before adding another.",
                    );
                }
                ui.separator();
                if ui.button("Close widget").clicked() {
                    actions.push(Action::CloseWidget);
                    ui.close();
                }
            });
        });
    });
    actions
}

fn row(ui: &mut egui::Ui, pin: &PinnedTask, clock: &Clock, actions: &mut Vec<Action>) {
    style::card().inner_margin(8).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 4.0;
        right_aligned_row(ui, |ui| {
            if ui.button("×").on_hover_text("Remove from widget").clicked() {
                actions.push(Action::UnpinEntry(pin.entry.id));
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(egui::Label::new(description(&pin.entry)).truncate())
                    .on_hover_text(description(&pin.entry));
            });
        });
        ui.spacing_mut().interact_size.y = 18.0;
        right_aligned_row(ui, |ui| {
            ui.monospace(crate::toggl::summary::clock(pin.elapsed(clock.now)));
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                if !pin.in_snapshot {
                    ui.add(
                        egui::Label::new(
                            RichText::new("Not in current history")
                                .size(11.0)
                                .color(style::MUTED),
                        )
                        .truncate(),
                    );
                } else if let Some(project) = &pin.project {
                    let colour = project_colour(project.color.as_deref());
                    dot(ui, colour);
                    let label = if pin.entry.is_running() {
                        format!("{} · Running", project.name)
                    } else {
                        project.name.clone()
                    };
                    ui.add(
                        egui::Label::new(RichText::new(&label).size(11.0).color(colour)).truncate(),
                    )
                    .on_hover_text(label);
                } else {
                    ui.label(
                        RichText::new(if pin.entry.is_running() {
                            "Running"
                        } else {
                            "No project"
                        })
                        .size(11.0)
                        .color(style::MUTED),
                    );
                }
            });
        });
    });
}

fn picker(
    ui: &mut egui::Ui,
    widget: &Widget,
    snapshot: &Snapshot,
    clock: &Clock,
    actions: &mut Vec<Action>,
) {
    ui.set_min_width(240.0);
    ui.set_max_width(260.0);
    ui.weak("Recent entries");
    if snapshot.entries.is_empty() {
        ui.weak("No entries are available.");
        return;
    }
    egui::ScrollArea::vertical()
        .id_salt("widget-picker")
        .max_height(140.0)
        .show(ui, |ui| {
            for entry in snapshot.entries.iter().take(20) {
                ui.push_id(entry.id, |ui| {
                    let project = entry
                        .project_id
                        .and_then(|id| snapshot.project(id))
                        .map(|project| project.name.as_str())
                        .unwrap_or("No project");
                    let when = entry
                        .start
                        .to_zoned(clock.tz.clone())
                        .strftime("%a %-d %b, %H:%M");
                    let label = format!("{}\n{project} · {when}", description(entry));
                    if pin_button(ui, widget, entry.id, &label).clicked() {
                        actions.push(Action::PinEntry(entry.id));
                        ui.close();
                    }
                });
            }
        });
}

fn pin_button(ui: &mut egui::Ui, widget: &Widget, id: i64, label: &str) -> egui::Response {
    let availability = widget.availability(id);
    let response = ui.add_enabled(
        availability == PinAvailability::Available,
        egui::Button::new(label).truncate(),
    );
    match availability {
        PinAvailability::Available => response,
        PinAvailability::AlreadyPinned => {
            response.on_disabled_hover_text("This entry is already in the widget.")
        }
        PinAvailability::Full => response.on_disabled_hover_text(
            "The widget has three tasks. Remove one before adding another.",
        ),
    }
}

pub(super) fn entry_action(ui: &mut egui::Ui, widget: &Widget, id: i64, actions: &mut Vec<Action>) {
    if pin_button(ui, widget, id, "Add to widget").clicked() {
        actions.push(Action::PinEntry(id));
        ui.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Connection, Toggl};
    use crate::toggl::{TimeEntry, User};

    fn state() -> State {
        let entries = (1..=4)
            .map(|id| TimeEntry {
                id,
                project_id: None,
                description: Some(format!("Task {id}")),
                start: "2026-10-16T12:00:00Z".parse().unwrap(),
                stop: None,
                duration: 600,
            })
            .collect();
        let mut state = State {
            toggl: Toggl {
                connection: Connection::SignedIn,
                snapshot: Some(Snapshot {
                    user: User {
                        fullname: None,
                        beginning_of_week: 1,
                    },
                    entries,
                    projects: Vec::new(),
                    workspaces: Vec::new(),
                    loaded_from: "2026-10-05T00:00:00Z".parse().unwrap(),
                    fetched_at: "2026-10-16T13:00:00Z".parse().unwrap(),
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        state.pin_entry(1);
        state
    }

    fn draw(
        ctx: &egui::Context,
        state: &State,
        events: Vec<egui::Event>,
    ) -> (Vec<Action>, egui::FullOutput) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(300.0, 304.0),
            )),
            events,
            ..Default::default()
        };
        let clock = Clock {
            now: "2026-10-16T13:00:00Z".parse().unwrap(),
            tz: jiff::tz::TimeZone::UTC,
        };
        let mut actions = Vec::new();
        let mut output = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| actions = show(ui, state, &clock));
        });
        output.textures_delta.clear();
        (actions, output)
    }

    fn text_position(output: &egui::FullOutput, prefix: &str) -> egui::Pos2 {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text().starts_with(prefix) => {
                    Some(text.pos + text.galley.rect.center().to_vec2())
                }
                _ => None,
            })
            .unwrap_or_else(|| {
                panic!(
                    "The expected control is not visible: {prefix}. Visible text: {:?}",
                    output
                        .shapes
                        .iter()
                        .filter_map(|shape| match &shape.shape {
                            egui::Shape::Text(text) => Some(text.galley.text()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                )
            })
    }

    fn click(pos: egui::Pos2) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            },
        ]
    }

    fn actions_position(output: &egui::FullOutput) -> Option<egui::Pos2> {
        output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Circle(circle) if circle.radius == 1.5 => Some(circle.center),
            _ => None,
        })
    }

    fn open_actions(ctx: &egui::Context, state: &State) -> egui::FullOutput {
        let (_, output) = draw(
            ctx,
            state,
            vec![egui::Event::PointerMoved(egui::pos2(20.0, 20.0))],
        );
        let menu = actions_position(&output).expect("The actions button must be visible.");
        draw(ctx, state, click(menu));
        draw(ctx, state, vec![]).1
    }

    #[test]
    fn actions_button_appears_on_hover_and_hides_when_the_pointer_leaves() {
        let ctx = egui::Context::default();
        crate::ui::install_fonts(&ctx);
        let state = state();
        let has_button = |output: &egui::FullOutput| actions_position(output).is_some();
        assert!(!has_button(&draw(&ctx, &state, vec![]).1));
        assert!(has_button(
            &draw(
                &ctx,
                &state,
                vec![egui::Event::PointerMoved(egui::pos2(20.0, 20.0))]
            )
            .1
        ));
        assert!(!has_button(
            &draw(&ctx, &state, vec![egui::Event::PointerGone]).1
        ));
    }

    #[test]
    fn picker_click_adds_the_selected_entry() {
        let ctx = egui::Context::default();
        crate::ui::install_fonts(&ctx);
        let state = state();
        let output = open_actions(&ctx, &state);
        let add = text_position(&output, "Add task");
        draw(&ctx, &state, click(add));
        let (_, output) = draw(&ctx, &state, vec![]);
        let task = text_position(&output, "Task 2");
        let (actions, _) = draw(&ctx, &state, click(task));
        assert!(
            actions
                .iter()
                .any(|action| matches!(action, Action::PinEntry(2)))
        );
    }

    #[test]
    fn actions_menu_can_close_a_full_widget() {
        let ctx = egui::Context::default();
        crate::ui::install_fonts(&ctx);
        let mut state = state();
        state.pin_entry(2);
        state.pin_entry(3);
        let output = open_actions(&ctx, &state);
        let add = text_position(&output, "Add task");
        let (actions, _) = draw(&ctx, &state, click(add));
        assert!(actions.is_empty());
        let output = open_actions(&ctx, &state);
        text_position(&output, "Close widget");
        let (_, output) = draw(&ctx, &state, vec![egui::Event::PointerGone]);
        assert!(actions_position(&output).is_some());
        let close = text_position(&output, "Close widget");
        let (actions, _) = draw(&ctx, &state, click(close));
        assert!(
            actions
                .iter()
                .any(|action| matches!(action, Action::CloseWidget))
        );
    }

    #[test]
    fn row_remove_click_returns_an_unpin_action() {
        let ctx = egui::Context::default();
        crate::ui::install_fonts(&ctx);
        let state = state();
        let (_, output) = draw(&ctx, &state, vec![]);
        let remove = text_position(&output, "×");
        let (actions, _) = draw(&ctx, &state, click(remove));
        assert!(
            actions
                .iter()
                .any(|action| matches!(action, Action::UnpinEntry(1)))
        );
    }

    #[test]
    fn pin_buttons_disable_duplicates_and_full_widgets() {
        let ctx = egui::Context::default();
        crate::ui::install_fonts(&ctx);
        let mut state = state();
        state.pin_entry(2);
        state.pin_entry(3);
        let mut enabled = [true; 2];
        let mut output = ctx.run_ui(Default::default(), |ui| {
            enabled[0] = pin_button(ui, &state.widget, 1, "Duplicate").enabled();
            enabled[1] = pin_button(ui, &state.widget, 4, "Full").enabled();
        });
        output.textures_delta.clear();
        assert_eq!(enabled, [false, false]);
        state.widget.remove(2);
        let mut output = ctx.run_ui(Default::default(), |ui| {
            enabled[1] = pin_button(ui, &state.widget, 4, "Available").enabled();
        });
        output.textures_delta.clear();
        assert!(enabled[1]);
    }
}
