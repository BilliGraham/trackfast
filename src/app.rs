use std::time::{Duration, Instant};

use eframe::egui;
use jiff::Timestamp;
use jiff::civil::Date;
use jiff::tz::TimeZone;

use crate::model::{Action, Connection, State};
use crate::sync::{Command, Event, Sync};
use crate::toggl::{ApiToken, summary};
use crate::ui;

pub struct App {
    state: State,
    sync: Sync,
    token_draft: String,
}

impl App {
    pub fn new(ctx: egui::Context) -> Self {
        Self {
            state: State::default(),
            sync: Sync::start(ctx),
            token_draft: String::new(),
        }
    }

    fn apply(&mut self, action: Action, ctx: &egui::Context) {
        match action {
            Action::Navigate(screen) => self.state.screen = screen,
            Action::OpenWidget => self.state.widget_open = true,
            Action::CloseWidget => self.state.widget_open = false,
            Action::SignIn => {
                if !self.state.toggl.can_sign_in(Instant::now()) {
                    return;
                }
                ui::clear_token_input(ctx);
                let draft = std::mem::take(&mut self.token_draft);
                match ApiToken::new(&draft) {
                    Some(token) => {
                        self.state.toggl.busy = true;
                        self.sync.send(Command::SignIn(token));
                    }
                    None => self.state.toggl.problem = Some("Paste your API token.".into()),
                }
            }
            Action::Refresh => {
                if self.state.toggl.can_refresh(Instant::now()) {
                    self.state.toggl.busy = true;
                    self.sync.send(Command::Refresh);
                }
            }
            Action::ShowTimerView(view) => self.state.timer_view = view,
            Action::ShowWeek(week) => {
                self.state.calendar.week = week.filter(|week| Some(*week) < self.current_week());
                self.load_shown_week();
            }
            Action::LoadWeek => self.load_shown_week(),
            Action::Zoom(step) => self.state.calendar.zoom(step),
            Action::SignOut => {
                self.state.calendar.week = None;
                self.token_draft.clear();
                ui::clear_token_input(ctx);
                self.state.toggl.connection = Connection::SignedOut;
                self.state.toggl.snapshot = None;
                self.state.toggl.busy = true;
                self.sync.send(Command::SignOut);
            }
        }
    }

    fn receive(&mut self, event: Event, ctx: &egui::Context) {
        let toggl = &mut self.state.toggl;
        match event {
            Event::SignedOut { problem } => {
                self.token_draft.clear();
                ui::clear_token_input(ctx);
                toggl.connection = Connection::SignedOut;
                toggl.busy = false;
                toggl.snapshot = None;
                toggl.problem = problem;
            }
            Event::Busy => toggl.busy = true,
            Event::RequestLimit(retry_at) => toggl.retry_at = retry_at,
            Event::Loaded { snapshot, problem } => {
                toggl.connection = Connection::SignedIn;
                toggl.busy = false;
                toggl.snapshot = Some(snapshot);
                toggl.problem = problem;
            }
            Event::EarlierLoaded { from, to, entries } => {
                toggl.busy = false;
                if let Some(snapshot) = &mut toggl.snapshot
                    && snapshot.loaded_from == to
                {
                    snapshot.extend_back(from, entries);
                }
                self.load_shown_week();
            }
            Event::Failed { problem, signed_in } => {
                toggl.connection = if signed_in {
                    Connection::SignedIn
                } else {
                    Connection::SignedOut
                };
                toggl.busy = false;
                toggl.problem = Some(problem);
            }
        }
    }

    fn current_week(&self) -> Option<Date> {
        let snapshot = self.state.toggl.snapshot.as_ref()?;
        let today = Timestamp::now().to_zoned(TimeZone::system()).date();
        Some(summary::week_start(today, snapshot.user.beginning_of_week))
    }

    /// Asks for the entries that the shown week needs and the snapshot lacks.
    fn load_shown_week(&mut self) {
        let toggl = &mut self.state.toggl;
        let (Some(week), Some(snapshot)) = (self.state.calendar.week, &toggl.snapshot) else {
            return;
        };
        let Some(from) = summary::start_of_day(week, &TimeZone::system()) else {
            return;
        };
        if toggl.busy || from >= snapshot.loaded_from {
            return;
        }
        toggl.busy = true;
        self.sync.send(Command::LoadEarlier {
            from,
            to: snapshot.loaded_from,
        });
    }

    fn schedule_repaint(&self, ctx: &egui::Context) {
        let toggl = &self.state.toggl;
        let running = toggl
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.running().is_some());
        if running {
            ctx.request_repaint_after(Duration::from_secs(1));
        }
        if let Some(wait) = toggl.request_wait(Instant::now()) {
            ctx.request_repaint_after(wait);
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        while let Some(event) = self.sync.try_recv() {
            self.receive(event, ui.ctx());
        }
        let clock = ui::Clock {
            now: Timestamp::now(),
            tz: TimeZone::system(),
        };
        for action in ui::shell(ui, &self.state, &mut self.token_draft, &clock) {
            self.apply(action, ui.ctx());
        }
        if self.state.widget_open {
            let mut close = false;
            ui.ctx().show_viewport_immediate(
                egui::ViewportId::from_hash_of("task-widget"),
                egui::ViewportBuilder::default()
                    .with_title("Trackfast widget")
                    .with_inner_size([350.0, 280.0])
                    .with_min_inner_size([300.0, 250.0])
                    .with_window_level(egui::WindowLevel::AlwaysOnTop),
                |widget_ui, _class| {
                    close = widget_ui.input(|input| input.viewport().close_requested());
                    egui::CentralPanel::default().show(widget_ui, ui::widget);
                },
            );
            if close {
                self.apply(Action::CloseWidget, ui.ctx());
            }
        }
        self.schedule_repaint(ui.ctx());
    }
}
