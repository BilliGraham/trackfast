mod actions;
mod calendar;
mod calendar_input;
mod day_blocks;
mod digits;
mod entry;
mod progress;
mod projects;
mod settings;
mod style;
mod widget;

pub use widget::{height as widget_height, show as widget};

use eframe::egui::{self, Color32, RichText};
use jiff::Timestamp;
use jiff::civil::Date;
use jiff::tz::TimeZone;

use crate::entries::{BEGINNING_OF_WEEK, Session, Snapshot, Task, summary};
use crate::model::{Action, Drafts, Screen, State, TimerView};

const NO_PROJECT: Color32 = Color32::from_rgb(110, 110, 110);

pub fn widget_background() -> Color32 {
    style::BACKGROUND
}

pub fn install_fonts(ctx: &egui::Context) {
    ctx.set_fonts(style::fonts());
}

pub fn clear_token_input(ctx: &egui::Context) {
    settings::clear_token_input(ctx);
}

pub struct Clock {
    pub now: Timestamp,
    pub tz: TimeZone,
}

impl Clock {
    fn today(&self) -> Date {
        self.now.to_zoned(self.tz.clone()).date()
    }

    fn local_time(&self, timestamp: Timestamp) -> String {
        timestamp
            .to_zoned(self.tz.clone())
            .strftime("%H:%M")
            .to_string()
    }
}

pub fn shell(ui: &mut egui::Ui, state: &State, drafts: &mut Drafts, clock: &Clock) -> Vec<Action> {
    style::apply(ui);
    let mut actions = Vec::new();
    if drafts.entry.is_none()
        && drafts.delete_entry.is_none()
        && !state.saving
        && state.data.is_some()
        && ui.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::N))
    {
        actions.push(Action::EditEntry(None));
    }
    egui::Panel::left("navigation")
        .exact_size(172.0)
        .resizable(false)
        .frame(
            egui::Frame::new()
                .fill(style::SIDEBAR)
                .inner_margin(16)
                .stroke(egui::Stroke::new(1.0, style::BORDER)),
        )
        .show(ui, |ui| navigation(ui, state, &mut actions));
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(style::BACKGROUND).inner_margin(24))
        .show(ui, |ui| {
            let Some(data) = &state.data else {
                match &state.problem {
                    Some(problem) => {
                        ui.colored_label(ui.visuals().warn_fg_color, problem);
                    }
                    None => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Opening your data");
                        });
                    }
                }
                return;
            };
            match state.screen {
                Screen::Timer => timer(ui, state, data, drafts, clock, &mut actions),
                Screen::Projects => projects::show(ui, state, data, drafts, &mut actions),
                Screen::Settings => settings::show(ui, state, drafts, &mut actions),
            }
        });
    entry::show(ui.ctx(), state, drafts, &mut actions);
    actions
}

fn navigation(ui: &mut egui::Ui, state: &State, actions: &mut Vec<Action>) {
    ui.add_space(8.0);
    ui.label(RichText::new("Trackfast").size(18.0).strong());
    ui.label(
        RichText::new("Time on this computer")
            .size(11.0)
            .color(style::MUTED),
    );
    ui.add_space(24.0);
    for (screen, label) in [
        (Screen::Timer, "Timer"),
        (Screen::Projects, "Projects"),
        (Screen::Settings, "Settings"),
    ] {
        if ui
            .add_sized(
                [ui.available_width(), 36.0],
                egui::Button::selectable(state.screen == screen, label),
            )
            .clicked()
        {
            actions.push(Action::Navigate(screen));
        }
    }
}

fn problem(ui: &mut egui::Ui, state: &State) {
    if let Some(problem) = &state.problem {
        ui.colored_label(ui.visuals().warn_fg_color, problem);
    }
}

fn timer(
    ui: &mut egui::Ui,
    state: &State,
    data: &Snapshot,
    drafts: &mut Drafts,
    clock: &Clock,
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        ui.heading("Timer");
        if ui.button("Add entry").clicked() {
            actions.push(Action::EditEntry(None));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (label, action) = if state.widget_open {
                ("Close widget", Action::CloseWidget)
            } else {
                ("Open widget", Action::OpenWidget)
            };
            if ui.button(label).clicked() {
                actions.push(action);
            }
        });
    });
    ui.add_space(16.0);
    running_card(ui, data, clock);
    ui.add_space(16.0);
    let today = clock.today();
    let current_week = summary::week_start(today, BEGINNING_OF_WEEK);
    let shown_week = state.calendar.week.unwrap_or(current_week);
    ui.horizontal(|ui| {
        match state.timer_view {
            TimerView::Calendar | TimerView::DayBlocks => {
                ui.scope(|ui| calendar::navigator(ui, shown_week, current_week, actions));
            }
            TimerView::List => {
                let total = |date| {
                    Some(summary::total_dates(
                        &data.sessions,
                        date,
                        today,
                        &clock.tz,
                        clock.now,
                    ))
                };
                ui.weak("Today");
                duration(ui, &clock_or_zero(total(today)));
                ui.add_space(16.0);
                ui.weak("This week");
                duration(ui, &clock_or_zero(total(current_week)));
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (view, label) in [
                (TimerView::List, "List"),
                (TimerView::DayBlocks, "Day blocks"),
                (TimerView::Calendar, "Calendar"),
            ] {
                if ui
                    .add(egui::Button::selectable(state.timer_view == view, label))
                    .clicked()
                {
                    actions.push(Action::ShowTimerView(view));
                }
            }
        });
    });
    problem(ui, state);
    if let Some(notice) = &state.notice {
        ui.weak(notice);
    }
    progress::show(
        ui,
        data,
        clock,
        if state.timer_view != TimerView::List {
            shown_week
        } else {
            current_week
        },
    );
    ui.add_space(12.0);
    match state.timer_view {
        TimerView::Calendar => calendar::show(ui, state, data, clock, shown_week, actions),
        TimerView::DayBlocks => day_blocks::show(ui, data, clock, shown_week, actions),
        TimerView::List => entry_list(ui, data, drafts, clock, today, actions),
    }
}

fn running_card(ui: &mut egui::Ui, data: &Snapshot, clock: &Clock) {
    let running: Vec<&Session> = data.running().collect();
    if running.is_empty() && ui.available_width() < 700.0 {
        ui.weak("No timer running. Add an entry to start a task or record completed work.");
        return;
    }
    style::card().inner_margin(16).show(ui, |ui| {
        ui.set_width(ui.available_width());
        if running.is_empty() {
            ui.label(RichText::new("No timer running").size(15.0));
            ui.weak("Add an entry to start a task or record completed work.");
            return;
        }
        for session in running {
            right_aligned_row(ui, |ui| {
                digits::label(
                    ui,
                    &summary::clock(session.elapsed(clock.now)),
                    digits::font(19.0),
                    style::TEXT,
                );
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let text = description(session);
                    ui.add(egui::Label::new(RichText::new(text).size(15.0)).truncate())
                        .on_hover_text(text);
                    ui.spacing_mut().interact_size.y = 18.0;
                    project_label(ui, data, session.project_id);
                });
            });
        }
    });
}

fn entry_list(
    ui: &mut egui::Ui,
    data: &Snapshot,
    drafts: &mut Drafts,
    clock: &Clock,
    today: Date,
    actions: &mut Vec<Action>,
) {
    ui.separator();
    let days = summary::days(&data.sessions, &clock.tz, clock.now);
    if days.is_empty() {
        ui.add_space(42.0);
        ui.label(RichText::new("No time tracked yet").size(16.0));
        ui.label("Add a completed entry or open the widget to start a timer.");
        return;
    }
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            for day in days {
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.strong(day_label(day.date, today));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        duration(ui, &summary::clock(day.total));
                    });
                });
                ui.add_space(4.0);
                for session in day.entries {
                    ui.push_id(session.id, |ui| {
                        entry_row(ui, data, session, drafts, clock, actions)
                    });
                }
            }
        });
}

fn duration(ui: &mut egui::Ui, text: &str) {
    digits::label(ui, text, digits::font(13.0), style::TEXT);
}

fn clock_or_zero(duration: Option<jiff::SignedDuration>) -> String {
    summary::clock(duration.unwrap_or_default())
}

fn entry_row(
    ui: &mut egui::Ui,
    data: &Snapshot,
    session: &Session,
    drafts: &mut Drafts,
    clock: &Clock,
    actions: &mut Vec<Action>,
) {
    style::card().show(ui, |ui| {
        ui.spacing_mut().interact_size.y = 18.0;
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.set_width(ui.available_width());
        right_aligned_row(ui, |ui| {
            self::actions::menu(ui, "Entry actions", |ui| {
                continue_action(ui, data, session, actions);
                if ui
                    .add_enabled(!session.is_running(), egui::Button::new("Edit entry"))
                    .clicked()
                {
                    actions.push(Action::EditEntry(Some(session.id)));
                    ui.close();
                }
                if ui
                    .add_enabled(!session.is_running(), egui::Button::new("Delete entry"))
                    .clicked()
                {
                    drafts.delete_entry = Some(session.id);
                    ui.close();
                }
            });
            duration(ui, &summary::clock(session.effective(clock.now)));
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(egui::Label::new(description(session)).truncate())
                    .on_hover_text(description(session));
            });
        });
        right_aligned_row(ui, |ui| {
            let end = match session.stop {
                Some(stop) => clock.local_time(stop),
                None => "now".to_owned(),
            };
            if session.details.duration_seconds.is_some() {
                ui.weak("Duration only");
            } else {
                ui.weak(format!("{} - {end}", clock.local_time(session.start)));
            }
            if session.details.round_to_five {
                ui.weak(format!(
                    "Recorded {}",
                    summary::clock(session.elapsed(clock.now))
                ))
                .on_hover_text("The total uses the entry's saved five-minute rounding rule.");
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                project_label(ui, data, session.project_id);
            });
        });
    });
}

/// The entry menu item that continues a session's task in the widget.
fn continue_action(
    ui: &mut egui::Ui,
    data: &Snapshot,
    session: &Session,
    actions: &mut Vec<Action>,
) {
    let button = ui.add_enabled(data.has_room(), egui::Button::new("Continue in widget"));
    let button = button
        .on_disabled_hover_text("The widget has three tasks. Remove one before adding another.");
    if button.clicked() {
        actions.push(Action::AddTask(Task {
            description: session.description.clone(),
            project_id: session.project_id,
        }));
        ui.close();
    }
}

fn description(session: &Session) -> &str {
    task_label(&session.description)
}

fn task_label(description: &str) -> &str {
    if description.trim().is_empty() {
        "(no description)"
    } else {
        description
    }
}

fn project_label(ui: &mut egui::Ui, data: &Snapshot, project_id: Option<i64>) {
    if let Some(project) = project_id.and_then(|id| data.project(id)) {
        let colour = project_colour(&project.colour);
        ui.horizontal(|ui| {
            dot(ui, colour);
            ui.add(egui::Label::new(RichText::new(&project.name).color(colour)).truncate())
                .on_hover_text(&project.name);
        });
    }
}

fn day_label(date: Date, today: Date) -> String {
    if date == today {
        "Today".to_owned()
    } else if today.yesterday().is_ok_and(|yesterday| yesterday == date) {
        "Yesterday".to_owned()
    } else {
        date.strftime("%a %-d %b").to_string()
    }
}

fn dot(ui: &mut egui::Ui, colour: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 5.0, colour);
}

/// Project colours use `#rrggbb`.
fn project_colour(hex: &str) -> Color32 {
    hex.strip_prefix('#')
        .filter(|digits| digits.len() == 6)
        .and_then(|digits| u32::from_str_radix(digits, 16).ok())
        .map(|rgb| {
            let [_, r, g, b] = rgb.to_be_bytes();
            Color32::from_rgb(r, g, b)
        })
        .unwrap_or(NO_PROJECT)
}

fn session_colour(data: &Snapshot, project_id: Option<i64>) -> Color32 {
    project_id
        .and_then(|id| data.project(id))
        .map(|project| project_colour(&project.colour))
        .unwrap_or(NO_PROJECT)
}

fn right_aligned_row(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), contents);
    });
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::entries::{Project, WidgetRow};
    use jiff::civil::date;

    #[test]
    fn project_colour_reads_hex() {
        assert_eq!(
            project_colour("#06aaf5"),
            Color32::from_rgb(0x06, 0xaa, 0xf5)
        );
        assert_eq!(project_colour("06aaf5"), NO_PROJECT);
        assert_eq!(project_colour("#xyz123"), NO_PROJECT);
    }

    #[test]
    fn day_labels_name_recent_days() {
        let today = date(2026, 10, 16);
        assert_eq!(day_label(today, today), "Today");
        assert_eq!(day_label(date(2026, 10, 15), today), "Yesterday");
        assert_eq!(day_label(date(2026, 10, 12), today), "Mon 12 Oct");
    }

    pub(crate) fn sample_snapshot() -> Snapshot {
        let session = |id, start: &str, seconds: Option<i64>, project_id| {
            let start: Timestamp = start.parse().unwrap();
            Session {
                details: Default::default(),
                id,
                description: "Review the calendar layout".into(),
                project_id,
                start,
                stop: seconds.map(|seconds| start + jiff::SignedDuration::from_secs(seconds)),
            }
        };
        let running = session(1, "2026-10-16T12:00:00Z", None, Some(3));
        Snapshot {
            projects: vec![Project {
                id: 3,
                name: "Site".into(),
                colour: "#06aaf5".into(),
                client_id: None,
                archived: false,
            }],
            settings: Default::default(),
            clients: Vec::new(),
            rows: vec![
                WidgetRow {
                    id: 10,
                    task: Task {
                        description: running.description.clone(),
                        project_id: Some(3),
                    },
                    session: Some(running.clone()),
                },
                WidgetRow {
                    id: 11,
                    task: Task {
                        description: "Support".into(),
                        project_id: None,
                    },
                    session: None,
                },
            ],
            sessions: vec![
                running,
                session(2, "2026-10-15T10:00:00Z", Some(3 * 3600), Some(3)),
                session(3, "2026-10-15T11:00:00Z", Some(3600), None),
                session(4, "2026-10-14T23:30:00Z", Some(3600), None),
                session(5, "2026-10-13T09:00:00Z", Some(60), Some(99)),
            ],
            loaded_from: "2026-10-05T00:00:00Z".parse().unwrap(),
            recent_tasks: vec![Task {
                description: "Support".into(),
                project_id: None,
            }],
        }
    }

    fn draw(ctx: &egui::Context, state: &State, drafts: &mut Drafts, time: f64) -> Vec<Action> {
        ctx.set_fonts(style::fonts());
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(760.0, 480.0),
            )),
            time: Some(time),
            ..Default::default()
        };
        let mut actions = Vec::new();
        let mut output = ctx.run_ui(input, |root| {
            let clock = Clock {
                now: "2026-10-16T13:00:00Z".parse().unwrap(),
                tz: jiff::tz::TimeZone::UTC,
            };
            actions = shell(root, state, drafts, &clock);
        });
        output.textures_delta.clear();
        actions
    }

    #[test]
    fn every_screen_draws_with_and_without_data() {
        let ctx = egui::Context::default();
        let mut state = State::default();
        let mut drafts = Drafts::default();
        draw(&ctx, &state, &mut drafts, 0.0);
        state.problem = Some("The local database failed.".into());
        draw(&ctx, &state, &mut drafts, 0.5);
        state.data = Some(sample_snapshot());
        let weeks = [None, Some(date(2026, 10, 5)), Some(date(2026, 9, 28))];
        for (step, week) in weeks.into_iter().enumerate() {
            state.calendar.week = week;
            for zoom in [0, 3] {
                state.calendar.zoom = zoom;
                draw(&ctx, &state, &mut drafts, 1.0 + step as f64);
            }
        }
        state.timer_view = TimerView::List;
        draw(&ctx, &state, &mut drafts, 10.0);
        for screen in [Screen::Projects, Screen::Settings] {
            state.screen = screen;
            draw(&ctx, &state, &mut drafts, 11.0);
        }
        drafts.project = Some(crate::db::ProjectDraft::default());
        state.screen = Screen::Projects;
        draw(&ctx, &state, &mut drafts, 12.0);
    }
}

#[cfg(test)]
mod entry_label_tests {
    use super::*;
    #[test]
    fn duration_rows_show_their_mode_and_preserved_recorded_time() {
        let ctx = egui::Context::default();
        ctx.set_fonts(style::fonts());
        let now = "2026-10-26T12:00:00Z".parse().unwrap();
        let clock = Clock {
            now,
            tz: TimeZone::UTC,
        };
        let mut session = tests::sample_snapshot().sessions[1].clone();
        session.details.duration_seconds = Some(780);
        session.details.round_to_five = true;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            entry_row(
                ui,
                &Snapshot::default(),
                &session,
                &mut Drafts::default(),
                &clock,
                &mut Vec::new(),
            )
        });
        let text = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some(t.galley.text()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");
        output.textures_delta.clear();
        assert!(text.contains("Duration only"), "{text}");
        assert!(text.contains("Recorded 0:13:00"), "{text}");
    }
}

#[cfg(test)]
mod day_view_tests {
    use super::*;
    #[test]
    fn day_blocks_show_durations_without_clock_ranges() {
        let ctx = egui::Context::default();
        ctx.set_fonts(style::fonts());
        let state = State {
            timer_view: TimerView::DayBlocks,
            data: Some(tests::sample_snapshot()),
            ..Default::default()
        };
        let mut drafts = Drafts::default();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1150.0, 720.0),
                )),
                ..Default::default()
            },
            |ui| {
                shell(
                    ui,
                    &state,
                    &mut drafts,
                    &Clock {
                        now: "2026-10-16T13:00:00Z".parse().unwrap(),
                        tz: TimeZone::UTC,
                    },
                );
            },
        );
        let text = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some(t.galley.text()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");
        output.textures_delta.clear();
        assert!(text.contains("Add duration"), "{text}");
        assert!(!text.contains("AM") && !text.contains("PM"));
    }
}
