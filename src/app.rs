use std::time::{Duration, Instant};

use eframe::egui;
use jiff::Timestamp;
use jiff::tz::TimeZone;

use crate::model::{Action, Connection, State};
use crate::sync::{Command, Event, Sync};
use crate::toggl::ApiToken;
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
            Action::SignOut => {
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
