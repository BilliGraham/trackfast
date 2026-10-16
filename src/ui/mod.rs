mod style;

use std::time::Instant;

use eframe::egui::{self, Color32, RichText};
use jiff::Timestamp;
use jiff::civil::Date;
use jiff::tz::TimeZone;

use crate::model::{Action, Connection, Screen, State, Toggl};
use crate::toggl::summary;
use crate::toggl::{Snapshot, TimeEntry};

const NO_PROJECT: Color32 = Color32::from_rgb(110, 110, 110);
const TOKEN_PAGE: &str = "https://track.toggl.com/profile";

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

/// `token_draft` is the text in the sign-in field. The view edits it in place
/// because egui text fields need a mutable buffer.
pub fn shell(
    ui: &mut egui::Ui,
    state: &State,
    token_draft: &mut String,
    clock: &Clock,
) -> Vec<Action> {
    style::apply(ui);
    let mut actions = Vec::new();
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
            let toggl = &state.toggl;
            match (state.screen, toggl.connection, &toggl.snapshot) {
                (_, Connection::Starting, _) => waiting(ui, "Reading the saved Toggl token"),
                (_, Connection::SignedOut, _) => sign_in(ui, toggl, token_draft, &mut actions),
                (_, Connection::SignedIn, None) if toggl.busy => {
                    waiting(ui, "Loading your Toggl data")
                }
                (_, Connection::SignedIn, None) => not_loaded(ui, toggl, &mut actions),
                (Screen::Timer, Connection::SignedIn, Some(snapshot)) => {
                    timer(ui, toggl, snapshot, clock, &mut actions)
                }
                (Screen::Projects, Connection::SignedIn, Some(snapshot)) => projects(ui, snapshot),
            }
        });
    actions
}

fn navigation(ui: &mut egui::Ui, state: &State, actions: &mut Vec<Action>) {
    ui.add_space(8.0);
    ui.label(RichText::new("Trackfast").size(18.0).strong());
    ui.label(RichText::new("Read only").size(11.0).color(style::MUTED));
    ui.add_space(24.0);
    if ui
        .add_sized(
            [ui.available_width(), 36.0],
            egui::Button::selectable(state.screen == Screen::Timer, "Timer"),
        )
        .clicked()
    {
        actions.push(Action::Navigate(Screen::Timer));
    }

    if ui
        .add_sized(
            [ui.available_width(), 36.0],
            egui::Button::selectable(state.screen == Screen::Projects, "Projects"),
        )
        .clicked()
    {
        actions.push(Action::Navigate(Screen::Projects));
    }
    ui.add_space(30.0);
    match (&state.toggl.connection, &state.toggl.snapshot) {
        (Connection::SignedIn, snapshot) => {
            let name = snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.user.fullname.as_deref())
                .unwrap_or("your account");
            ui.weak(format!("Connected to Toggl as {name}."));
            if ui
                .add_enabled(!state.toggl.busy, egui::Button::new("Sign out"))
                .clicked()
            {
                actions.push(Action::SignOut);
            }
        }
        _ => {
            ui.weak("Toggl is not connected.");
        }
    }
}

fn waiting(ui: &mut egui::Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.spinner();
        ui.label(text);
    });
}

fn problem(ui: &mut egui::Ui, toggl: &Toggl) {
    if let Some(problem) = &toggl.problem {
        ui.colored_label(ui.visuals().warn_fg_color, problem);
    }
}

fn token_field_id() -> egui::Id {
    egui::Id::new("toggl-api-token")
}

pub fn clear_token_input(ctx: &egui::Context) {
    ctx.data_mut(|data| data.remove::<egui::text_edit::TextEditState>(token_field_id()));
}

fn sign_in(ui: &mut egui::Ui, toggl: &Toggl, token_draft: &mut String, actions: &mut Vec<Action>) {
    ui.heading("Connect to Toggl Track");
    ui.set_max_width(420.0);
    ui.add_space(8.0);
    ui.label("Paste the API token from your Toggl Track profile.");
    ui.hyperlink_to("Open your Toggl profile", TOKEN_PAGE);
    ui.add_space(12.0);
    let field = ui.add_enabled(
        !toggl.busy,
        egui::TextEdit::singleline(token_draft)
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
    let can_connect = toggl.can_sign_in(Instant::now());
    let submitted =
        can_connect && field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let connect = ui.add_enabled(can_connect, egui::Button::new("Connect"));
        let connect = request_wait_hint(connect, toggl);
        if connect.clicked() || submitted {
            actions.push(Action::SignIn);
        }
        if toggl.busy {
            ui.spinner();
        }
    });
    ui.add_space(8.0);
    problem(ui, toggl);
    ui.add_space(12.0);
    ui.weak("Trackfast keeps the token in the credential store of your operating system.");
    ui.weak("Trackfast only reads data. It does not change anything in Toggl.");
}

fn not_loaded(ui: &mut egui::Ui, toggl: &Toggl, actions: &mut Vec<Action>) {
    ui.heading("Toggl data is not loaded");
    ui.add_space(8.0);
    problem(ui, toggl);
    ui.add_space(8.0);
    refresh_button(ui, toggl, "Try again", actions);
}

fn refresh_button(ui: &mut egui::Ui, toggl: &Toggl, label: &str, actions: &mut Vec<Action>) {
    let now = Instant::now();
    let response = ui.add_enabled(toggl.can_refresh(now), egui::Button::new(label));
    let response = request_wait_hint(response, toggl);
    if response.clicked() {
        actions.push(Action::Refresh);
    }
}

fn request_wait_hint(response: egui::Response, toggl: &Toggl) -> egui::Response {
    match toggl.request_wait(Instant::now()) {
        Some(wait) => {
            response.on_disabled_hover_text(format!("Try again in {} seconds.", wait.as_secs() + 1))
        }
        None => response,
    }
}

fn timer(
    ui: &mut egui::Ui,
    toggl: &Toggl,
    snapshot: &Snapshot,
    clock: &Clock,
    actions: &mut Vec<Action>,
) {
    let running = snapshot.running();
    ui.horizontal(|ui| {
        ui.heading("Timer");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Open widget").clicked() {
                actions.push(Action::OpenWidget);
            }
        });
    });
    ui.add_space(16.0);
    style::card().inner_margin(16).show(ui, |ui| {
        ui.set_width(ui.available_width());
        right_aligned_row(ui, |ui| {
            ui.add_enabled(false, egui::Button::new("Start"))
                .on_disabled_hover_text("Timer control is not available yet.");
            let elapsed = running.map(|entry| entry.elapsed(clock.now));
            ui.label(RichText::new(clock_or_zero(elapsed)).monospace().size(19.0));
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                let text = running.map(description).unwrap_or("No timer running");
                ui.add(egui::Label::new(RichText::new(text).size(15.0)).truncate())
                    .on_hover_text(text);
            });
        });
        if let Some(entry) = running {
            ui.spacing_mut().interact_size.y = 18.0;
            project_label(ui, snapshot, entry);
        }
    });
    ui.add_space(16.0);
    let today = clock.today();
    let week = summary::week_start(today, snapshot.user.beginning_of_week);
    let total = |date| {
        summary::start_of_day(date, &clock.tz)
            .map(|since| summary::total_since(&snapshot.entries, since, clock.now))
    };
    ui.horizontal_wrapped(|ui| {
        ui.weak("Today");
        ui.monospace(clock_or_zero(total(today)));
        ui.add_space(16.0);
        ui.weak("This week");
        ui.monospace(clock_or_zero(total(week)));
    });
    ui.add_space(12.0);
    ui.separator();
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.strong("Time entries");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            refresh_button(ui, toggl, "Refresh", actions);
            if toggl.busy {
                ui.spinner();
            } else {
                ui.weak(format!("Synced {}", clock.local_time(snapshot.fetched_at)));
            }
        });
    });
    problem(ui, toggl);
    ui.add_space(12.0);
    ui.separator();
    let days = summary::days(&snapshot.entries, &clock.tz, clock.now);
    if days.is_empty() {
        ui.add_space(42.0);
        ui.label(RichText::new("No recent time entries").size(16.0));
        ui.label("Entries that you track in Toggl will appear here after a refresh.");
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
                        ui.monospace(summary::clock(day.total));
                    });
                });
                ui.add_space(4.0);
                for entry in day.entries {
                    entry_row(ui, snapshot, entry, clock);
                }
            }
        });
}

fn clock_or_zero(duration: Option<jiff::SignedDuration>) -> String {
    summary::clock(duration.unwrap_or_default())
}

fn entry_row(ui: &mut egui::Ui, snapshot: &Snapshot, entry: &TimeEntry, clock: &Clock) {
    style::card().show(ui, |ui| {
        ui.spacing_mut().interact_size.y = 18.0;
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.set_width(ui.available_width());
        right_aligned_row(ui, |ui| {
            ui.monospace(summary::clock(entry.elapsed(clock.now)));
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(egui::Label::new(description(entry)).truncate())
                    .on_hover_text(description(entry));
            });
        });
        right_aligned_row(ui, |ui| {
            let end = match entry.stop {
                Some(stop) if !entry.is_running() => clock.local_time(stop),
                _ => "now".to_owned(),
            };
            ui.weak(format!("{} – {end}", clock.local_time(entry.start)));
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                project_label(ui, snapshot, entry);
            });
        });
    });
}

fn description(entry: &TimeEntry) -> &str {
    match entry.description.as_deref() {
        Some(text) if !text.trim().is_empty() => text,
        _ => "(no description)",
    }
}

fn project_label(ui: &mut egui::Ui, snapshot: &Snapshot, entry: &TimeEntry) {
    if let Some(project) = entry.project_id.and_then(|id| snapshot.project(id)) {
        let colour = project_colour(project.color.as_deref());
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

fn projects(ui: &mut egui::Ui, snapshot: &Snapshot) {
    ui.horizontal(|ui| {
        ui.heading("Projects");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_enabled(false, egui::Button::new("New project"))
                .on_disabled_hover_text("Project creation is not available yet.");
        });
    });
    ui.add_space(12.0);
    ui.separator();
    if snapshot.projects.is_empty() {
        ui.add_space(20.0);
        ui.label(RichText::new("No projects").size(16.0));
        ui.label("Projects that you create in Toggl will appear here after a refresh.");
        return;
    }
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            for workspace in &snapshot.workspaces {
                let mut projects: Vec<_> = snapshot
                    .projects
                    .iter()
                    .filter(|project| project.workspace_id == workspace.id)
                    .collect();
                if projects.is_empty() {
                    continue;
                }
                projects.sort_by_key(|project| (!project.active, project.name.to_lowercase()));
                ui.add_space(16.0);
                ui.strong(&workspace.name);
                ui.add_space(4.0);
                for project in projects {
                    ui.horizontal(|ui| {
                        dot(ui, project_colour(project.color.as_deref()));
                        ui.label(&project.name);
                        if !project.active {
                            ui.weak("archived");
                        }
                    });
                }
            }
        });
}

fn dot(ui: &mut egui::Ui, colour: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 5.0, colour);
}

/// Toggl sends project colours as `#rrggbb`.
fn project_colour(hex: Option<&str>) -> Color32 {
    hex.and_then(|hex| hex.strip_prefix('#'))
        .filter(|digits| digits.len() == 6)
        .and_then(|digits| u32::from_str_radix(digits, 16).ok())
        .map(|rgb| {
            let [_, r, g, b] = rgb.to_be_bytes();
            Color32::from_rgb(r, g, b)
        })
        .unwrap_or(NO_PROJECT)
}

pub fn widget(ui: &mut egui::Ui) {
    style::apply(ui);
    ui.spacing_mut().button_padding = egui::vec2(10.0, 4.0);
    ui.spacing_mut().interact_size.y = 26.0;
    ui.label(RichText::new("Pinned tasks").size(16.0).strong());
    ui.add_space(4.0);
    for slot in 1..=3 {
        style::card().inner_margin(8).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(format!("Task {slot}"));
                    ui.label(
                        RichText::new("No task selected")
                            .size(11.0)
                            .color(style::MUTED),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_enabled(false, egui::Button::new("Start"))
                        .on_disabled_hover_text("Timer controls are not available yet.");
                    ui.monospace("0:00:00");
                });
            });
        });
    }
}

fn right_aligned_row(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), contents);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    #[test]
    fn project_colour_reads_toggl_hex() {
        assert_eq!(
            project_colour(Some("#06aaf5")),
            Color32::from_rgb(0x06, 0xaa, 0xf5)
        );
        assert_eq!(project_colour(Some("06aaf5")), NO_PROJECT);
        assert_eq!(project_colour(Some("#xyz123")), NO_PROJECT);
        assert_eq!(project_colour(None), NO_PROJECT);
    }

    #[test]
    fn day_labels_name_recent_days() {
        let today = date(2026, 10, 16);
        assert_eq!(day_label(today, today), "Today");
        assert_eq!(day_label(date(2026, 10, 15), today), "Yesterday");
        assert_eq!(day_label(date(2026, 10, 12), today), "Mon 12 Oct");
    }
    fn draw(
        ctx: &egui::Context,
        state: &State,
        draft: &mut String,
        time: f64,
        events: Vec<egui::Event>,
    ) {
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(760.0, 480.0));
        let input = egui::RawInput {
            screen_rect: Some(rect),
            time: Some(time),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |root| {
            let clock = Clock {
                now: "2026-10-16T13:00:00Z".parse().unwrap(),
                tz: jiff::tz::TimeZone::UTC,
            };
            shell(root, state, draft, &clock);
        });
        output.textures_delta.clear();
    }
    #[test]
    fn token_undo_cannot_restore_submitted_credentials() {
        let ctx = egui::Context::default();
        ctx.set_visuals(egui::Visuals::dark());
        let mut state = State::default();
        state.toggl.connection = Connection::SignedOut;
        let mut draft = String::new();
        draw(&ctx, &state, &mut draft, 0.0, vec![]);
        ctx.memory_mut(|memory| memory.request_focus(token_field_id()));
        draw(&ctx, &state, &mut draft, 0.1, vec![]);
        assert!(
            ctx.memory(|m| m.focused().is_some()),
            "The synthetic token field did not receive focus."
        );
        draw(
            &ctx,
            &state,
            &mut draft,
            0.2,
            vec![egui::Event::Paste("synthetic-review-token".into())],
        );
        assert_eq!(draft, "synthetic-review-token");
        draw(&ctx, &state, &mut draft, 1.5, vec![]);
        draw(&ctx, &state, &mut draft, 2.0, vec![]);
        let _submitted = std::mem::take(&mut draft);
        state.toggl.connection = Connection::SignedIn;
        draw(&ctx, &state, &mut draft, 3.0, vec![]);
        state.toggl.connection = Connection::SignedOut;
        draw(&ctx, &state, &mut draft, 4.0, vec![]);
        ctx.memory_mut(|memory| memory.request_focus(token_field_id()));
        draw(&ctx, &state, &mut draft, 4.1, vec![]);
        let command = egui::Modifiers {
            ctrl: true,
            command: true,
            ..Default::default()
        };
        draw(
            &ctx,
            &state,
            &mut draft,
            4.2,
            vec![egui::Event::Key {
                key: egui::Key::Z,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: command,
            }],
        );
        assert!(
            draft.is_empty(),
            "The password undo history restored a submitted token."
        );
    }
}
