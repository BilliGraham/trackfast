use eframe::egui::{self, Color32, RichText, Sense, pos2, vec2};

use super::{Clock, digits, project_colour, session_colour, style, task_label};
use crate::entries::{Snapshot, Task, WidgetRow, summary};
use crate::model::{Action, Drafts, State};

const BUTTON_SIZE: f32 = 28.0;

pub fn height(rows: usize) -> f32 {
    match rows {
        0 | 1 => 160.0,
        2 => 232.0,
        _ => 304.0,
    }
}

pub fn show(ui: &mut egui::Ui, state: &State, drafts: &mut Drafts, clock: &Clock) -> Vec<Action> {
    style::apply(ui);
    ui.spacing_mut().button_padding = egui::vec2(10.0, 4.0);
    ui.spacing_mut().interact_size.y = 26.0;
    ui.spacing_mut().item_spacing.y = 6.0;
    let mut actions = Vec::new();
    let Some(data) = &state.data else {
        ui.spinner();
        return actions;
    };
    let show_menu = ui
        .ctx()
        .pointer_hover_pos()
        .is_some_and(|pos| ui.max_rect().contains(pos))
        || egui::Popup::is_any_open(ui.ctx())
        || ui.ctx().memory(|memory| memory.focused().is_some());
    egui::ScrollArea::vertical()
        .id_salt("widget-rows")
        .auto_shrink([false, true])
        .max_height((ui.available_height() - 36.0).max(32.0))
        .show(ui, |ui| {
            if data.rows.is_empty() {
                style::card().inner_margin(8).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label("No tasks");
                    ui.weak("Open the menu below and add a task to start a timer.");
                });
            }
            for row in &data.rows {
                ui.push_id(row.id, |ui| task_row(ui, data, row, clock, &mut actions));
            }
        });
    if let Some(problem) = &state.problem {
        ui.colored_label(ui.visuals().warn_fg_color, problem);
    }
    ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
        ui.scope(|ui| {
            if !show_menu {
                ui.set_opacity(0.0);
            }
            super::actions::menu(ui, "Widget actions", |ui| {
                let add = ui.add_enabled_ui(data.has_room(), |ui| {
                    ui.menu_button("Add task", |ui| {
                        add_task_menu(ui, data, drafts, &mut actions);
                    })
                });
                add.inner.response.on_disabled_hover_text(
                    "The widget has three tasks. Remove one before adding another.",
                );
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

fn task_row(
    ui: &mut egui::Ui,
    data: &Snapshot,
    row: &WidgetRow,
    clock: &Clock,
    actions: &mut Vec<Action>,
) {
    let colour = session_colour(data, row.task.project_id);
    style::card().inner_margin(8).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let running = row.session.is_some();
            let toggle = play_pause(ui, running, colour).on_hover_text(if running {
                "Pause"
            } else {
                "Start a new session"
            });
            if toggle.clicked() {
                actions.push(if running {
                    Action::Pause(row.id)
                } else {
                    Action::Play(row.id)
                });
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("×")
                    .on_hover_text("Remove from widget")
                    .clicked()
                {
                    actions.push(Action::RemoveRow(row.id));
                }
                let time = match &row.session {
                    Some(session) => session.elapsed(clock.now),
                    None => summary::start_of_day(clock.today(), &clock.tz)
                        .map(|today| data.task_total(&row.task, today, clock.now))
                        .unwrap_or_default(),
                };
                let text_colour = if running { style::TEXT } else { style::MUTED };
                digits::label(ui, &summary::clock(time), digits::font(15.0), text_colour)
                    .on_hover_text(if running {
                        "This session"
                    } else {
                        "Today's total for this task"
                    });
                ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let label = task_label(&row.task.description);
                    ui.add(egui::Label::new(RichText::new(label).size(13.0)).truncate())
                        .on_hover_text(label);
                    let project = row
                        .task
                        .project_id
                        .and_then(|id| data.project(id))
                        .map_or("No project", |project| project.name.as_str());
                    ui.add(
                        egui::Label::new(RichText::new(project).size(11.0).color(colour))
                            .truncate(),
                    );
                });
            });
        });
    });
}

/// Draws a round play or pause button. The bundled fonts lack these glyphs.
fn play_pause(ui: &mut egui::Ui, running: bool, colour: Color32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(BUTTON_SIZE, BUTTON_SIZE), Sense::click());
    let painter = ui.painter_at(rect);
    let centre = rect.center();
    let fill = if response.hovered() {
        colour
    } else {
        colour.gamma_multiply(0.8)
    };
    painter.circle_filled(centre, BUTTON_SIZE / 2.0, fill);
    let icon = Color32::WHITE;
    if running {
        for x in [-3.5, 3.5] {
            painter.rect_filled(
                egui::Rect::from_center_size(centre + vec2(x, 0.0), vec2(3.5, 11.0)),
                1.0,
                icon,
            );
        }
    } else {
        painter.add(egui::Shape::convex_polygon(
            vec![
                pos2(centre.x - 3.5, centre.y - 6.5),
                pos2(centre.x + 6.5, centre.y),
                pos2(centre.x - 3.5, centre.y + 6.5),
            ],
            icon,
            egui::Stroke::NONE,
        ));
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            if running { "Pause" } else { "Play" },
        )
    });
    response
}

fn add_task_menu(
    ui: &mut egui::Ui,
    data: &Snapshot,
    drafts: &mut Drafts,
    actions: &mut Vec<Action>,
) {
    ui.set_min_width(240.0);
    ui.set_max_width(260.0);
    ui.weak("New task");
    let field = ui.add(
        egui::TextEdit::singleline(&mut drafts.task.description)
            .hint_text("What are you working on?")
            .desired_width(240.0),
    );
    let selected = drafts
        .task
        .project_id
        .and_then(|id| data.project(id))
        .map_or("No project", |project| project.name.as_str());
    egui::ComboBox::from_id_salt("new-task-project")
        .selected_text(selected)
        .width(240.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut drafts.task.project_id, None, "No project");
            for project in data.projects.iter().filter(|project| !project.archived) {
                ui.selectable_value(
                    &mut drafts.task.project_id,
                    Some(project.id),
                    RichText::new(&project.name).color(project_colour(&project.colour)),
                );
            }
        });
    let submitted = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
    if ui.button("Start").clicked() || submitted {
        actions.push(Action::AddDraftTask);
        ui.close();
    }
    let recent: Vec<&Task> = data
        .recent_tasks
        .iter()
        .filter(|task| data.rows.iter().all(|row| &row.task != *task))
        .collect();
    if recent.is_empty() {
        return;
    }
    ui.separator();
    ui.weak("Continue a recent task");
    egui::ScrollArea::vertical()
        .id_salt("widget-recent")
        .max_height(160.0)
        .show(ui, |ui| {
            for (index, task) in recent.into_iter().enumerate() {
                ui.push_id(index, |ui| {
                    let project = task
                        .project_id
                        .and_then(|id| data.project(id))
                        .map_or("No project", |project| project.name.as_str());
                    let label = format!("{}\n{project}", task_label(&task.description));
                    if ui.add(egui::Button::new(label).truncate()).clicked() {
                        actions.push(Action::AddTask(task.clone()));
                        ui.close();
                    }
                });
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tests::sample_snapshot;

    fn draw(
        ctx: &egui::Context,
        state: &State,
        drafts: &mut Drafts,
        events: Vec<egui::Event>,
    ) -> (Vec<Action>, egui::FullOutput) {
        crate::ui::install_fonts(ctx);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(320.0, 304.0),
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
            egui::CentralPanel::default().show(ui, |ui| actions = show(ui, state, drafts, &clock));
        });
        output.textures_delta.clear();
        (actions, output)
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

    fn buttons(output: &egui::FullOutput) -> Vec<egui::Pos2> {
        output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Circle(circle) if circle.radius == BUTTON_SIZE / 2.0 => {
                    Some(circle.center)
                }
                _ => None,
            })
            .collect()
    }

    fn state() -> State {
        State {
            data: Some(sample_snapshot()),
            widget_open: true,
            ..Default::default()
        }
    }

    #[test]
    fn running_rows_pause_and_paused_rows_play() {
        let ctx = egui::Context::default();
        let state = state();
        let mut drafts = Drafts::default();
        let (_, output) = draw(&ctx, &state, &mut drafts, vec![]);
        let toggles = buttons(&output);
        assert_eq!(toggles.len(), 2);
        let (actions, _) = draw(&ctx, &state, &mut drafts, click(toggles[0]));
        assert!(matches!(actions.as_slice(), [Action::Pause(10)]));
        let (actions, _) = draw(&ctx, &state, &mut drafts, click(toggles[1]));
        assert!(matches!(actions.as_slice(), [Action::Play(11)]));
    }

    #[test]
    fn the_widget_shows_tasks_but_no_history() {
        let ctx = egui::Context::default();
        let (_, output) = draw(&ctx, &state(), &mut Drafts::default(), vec![]);
        let texts: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                _ => None,
            })
            .collect();
        assert!(texts.iter().any(|text| text == "Support"));
        assert_eq!(
            texts
                .iter()
                .filter(|text| text.starts_with("Review the calendar layout"))
                .count(),
            1
        );
    }
}
