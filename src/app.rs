use std::sync::mpsc::Receiver;
use std::time::Duration;

use eframe::egui;
use jiff::Timestamp;
use jiff::civil::Date;
use jiff::tz::TimeZone;

use crate::db::ProjectDraft;
use crate::db::worker::{Command, Database, Event, Events};
use crate::entries::{BEGINNING_OF_WEEK, Task, summary, week};
use crate::import::{self, Progress};
use crate::model::{Action, Drafts, ImportState, State};
use crate::toggl::ApiToken;
use crate::ui;

pub struct App {
    account: crate::account::Account,
    state: State,
    drafts: Drafts,
    database: Option<(Database, Events)>,
    import: Option<Receiver<Progress>>,
}

fn current_week(tz: &TimeZone) -> Date {
    let today = Timestamp::now().to_zoned(tz.clone()).date();
    summary::week_start(today, BEGINNING_OF_WEEK)
}

impl App {
    pub fn new(ctx: egui::Context) -> Self {
        let account = crate::account::Account::start(ctx.clone());
        let tz = TimeZone::system();
        let mut state = State::default();
        let loaded_from = summary::start_of_day(week::shift(current_week(&tz), -1), &tz)
            .unwrap_or(Timestamp::UNIX_EPOCH);
        let database = match crate::db::default_path() {
            Some(path) => Some(Database::start(path, loaded_from, ctx)),
            None => {
                state.problem = Some("Trackfast cannot find a data folder for this user.".into());
                None
            }
        };
        let default_import_from = week::shift(current_week(&tz), -12);
        Self {
            account,
            state,
            drafts: Drafts {
                import_from: default_import_from.to_string(),
                ..Drafts::default()
            },
            database,
            import: None,
        }
    }

    fn send(&self, command: Command) {
        if let Some((database, _)) = &self.database {
            database.send(command);
        }
    }

    fn apply(&mut self, action: Action, ctx: &egui::Context) {
        let now = Timestamp::now();
        self.state.notice = None;
        // Without data, the problem explains why the app cannot open, so it stays.
        if self.state.data.is_some() {
            self.state.problem = None;
        }
        match action {
            Action::SignIn => {
                if !self.state.account.status.busy()
                    && self.state.account.status != crate::account::Status::Connected
                {
                    let secret = std::mem::take(&mut self.drafts.account_secret);
                    ui::clear_secret_input(ctx);
                    self.state.account.status = crate::account::Status::SigningIn;
                    self.state.account.problem = None;
                    self.account.send(crate::account::Command::SignIn(secret));
                }
            }
            Action::CancelSignIn => self.account.cancel(),
            Action::RestoreAccount => {
                if !self.state.account.status.busy() {
                    self.state.account.status = crate::account::Status::Checking;
                    self.account.send(crate::account::Command::Restore);
                }
            }
            Action::SignOut => {
                if !self.state.account.status.busy() {
                    use zeroize::Zeroize;
                    self.drafts.account_secret.zeroize();
                    ui::clear_secret_input(ctx);
                    self.state.account.status = crate::account::Status::SigningOut;
                    self.account.send(crate::account::Command::SignOut);
                }
            }
            Action::DraftEntry(draft) => {
                if !self.state.saving && self.drafts.entry.is_none() {
                    self.drafts.entry = Some(*draft);
                }
            }
            Action::StartEntry => {
                if self.state.saving {
                    return;
                }
                if let Some(draft) = &self.drafts.entry {
                    if draft.description.trim().is_empty() {
                        self.state.problem = Some("Enter what you are working on.".into());
                        return;
                    }
                    if !self.state.data.as_ref().is_some_and(|data| data.has_room()) {
                        self.state.problem = Some(
                            "The widget has three tasks. Remove one before starting another."
                                .into(),
                        );
                        return;
                    }
                    self.state.saving = true;
                    self.send(Command::StartEntry(
                        Task {
                            description: draft.description.trim().into(),
                            project_id: draft.project_id,
                        },
                        now,
                    ));
                }
            }

            Action::EditEntry(id) => {
                if self.state.saving {
                    return;
                }
                self.drafts.entry = Some(
                    id.and_then(|id| {
                        self.state
                            .data
                            .as_ref()?
                            .sessions
                            .iter()
                            .find(|s| s.id == id)
                    })
                    .map(|s| crate::model::EntryDraft::from_session(s, &TimeZone::system()))
                    .unwrap_or_else(|| crate::model::EntryDraft::new(now, &TimeZone::system())),
                );
            }
            Action::CancelEntry => {
                if !self.state.saving {
                    self.drafts.entry = None;
                    self.drafts.delete_entry = None;
                }
            }
            Action::SaveEntry => {
                if self.state.saving {
                    return;
                }
                if let Some(draft) = &self.drafts.entry {
                    match draft.session() {
                        Ok(entry) => {
                            self.state.saving = true;
                            self.send(Command::SaveEntry(entry));
                        }
                        Err(error) => self.state.problem = Some(error),
                    }
                }
            }
            Action::DeleteEntry(id) => {
                if !self.state.saving {
                    self.state.saving = true;
                    self.send(Command::DeleteEntry(id));
                }
            }
            Action::SaveSettings => {
                if !self.state.saving
                    && let Some(settings) = &self.drafts.settings
                {
                    self.state.saving = true;
                    self.send(Command::SaveSettings(settings.clone()));
                }
            }

            Action::Navigate(screen) => self.state.screen = screen,
            Action::OpenWidget => self.state.widget_open = true,
            Action::CloseWidget => self.state.widget_open = false,
            Action::ShowTimerView(view) => self.state.timer_view = view,
            Action::Zoom(step) => self.state.calendar.zoom(step),
            Action::ShowWeek(week) => {
                let tz = TimeZone::system();
                self.state.calendar.week = week.filter(|week| *week < current_week(&tz));
                if let Some(from) = week.and_then(|week| summary::start_of_day(week, &tz)) {
                    self.send(Command::LoadFrom(from));
                }
            }
            Action::AddTask(task) => self.add_task(task, now),
            Action::AddDraftTask => {
                let mut task = std::mem::take(&mut self.drafts.task);
                task.description = task.description.trim().to_owned();
                if task.description.is_empty() {
                    self.state.problem = Some("Enter what you are working on.".into());
                    self.drafts.task = task;
                } else {
                    self.add_task(task, now);
                }
            }
            Action::Play(row) => self.send(Command::Play(row, now)),
            Action::Pause(row) => self.send(Command::Pause(row, now)),
            Action::RemoveRow(row) => self.send(Command::RemoveRow(row, now)),
            Action::EditProject(id) => {
                if let Some(data) = &self.state.data {
                    self.drafts.project = Some(ProjectDraft::for_project(data, id));
                }
            }
            Action::CancelProject => self.drafts.project = None,
            Action::SaveProject => {
                let Some(draft) = self.drafts.project.take() else {
                    return;
                };
                if draft.name.trim().is_empty() {
                    self.state.problem = Some("Enter a project name.".into());
                    self.drafts.project = Some(draft);
                } else {
                    self.send(Command::SaveProject(draft));
                }
            }
            Action::SetArchived(project, archived) => {
                self.send(Command::SetArchived(project, archived))
            }
            Action::StartImport => self.start_import(ctx),
        }
    }

    fn add_task(&mut self, task: Task, now: Timestamp) {
        let has_room = self.state.data.as_ref().is_some_and(|data| data.has_room());
        if !has_room {
            self.state.problem =
                Some("The widget has three tasks. Remove one before adding another.".into());
            return;
        }
        self.state.problem = None;
        self.state.widget_open = true;
        self.send(Command::AddTask(task, now));
    }

    fn start_import(&mut self, ctx: &egui::Context) {
        if matches!(self.state.import, ImportState::Running(_)) {
            return;
        }
        let Some((database, _)) = &self.database else {
            return;
        };
        let token = std::mem::take(&mut self.drafts.import_token);
        ui::clear_token_input(ctx);
        let Some(token) = ApiToken::new(&token) else {
            self.state.import = ImportState::Failed("Paste your Toggl API token.".into());
            return;
        };
        let from = self
            .drafts
            .import_from
            .trim()
            .parse::<Date>()
            .ok()
            .and_then(|date| summary::start_of_day(date, &TimeZone::system()));
        let Some(from) = from else {
            self.state.import = ImportState::Failed("Enter the start date as YYYY-MM-DD.".into());
            return;
        };
        self.state.import = ImportState::Running("Connecting to Toggl".into());
        self.import = Some(import::start(token, from, database.clone(), ctx.clone()));
    }

    fn receive(&mut self, ctx: &egui::Context) {
        while let Some(event) = self.account.receive() {
            match event {
                crate::account::Event::State(view) => self.state.account = view,
                crate::account::Event::Browser(url) => {
                    if self.account.browser_allowed() {
                        ctx.open_url(egui::OpenUrl::new_tab(url));
                    }
                }
            }
        }
        if let Some((_, events)) = &self.database {
            while let Some(event) = events.try_recv() {
                match event {
                    Event::Saved(saved) => {
                        self.state.saving = false;
                        self.state.notice = Some("Saved on this computer.".into());
                        match saved {
                            crate::db::worker::Saved::Entry => {
                                if let Some(draft) = self.drafts.entry.take()
                                    && let Ok(entry) = draft.session()
                                {
                                    self.send(Command::LoadFrom(entry.start));
                                }
                            }
                            crate::db::worker::Saved::Started => {
                                self.drafts.entry = None;
                                self.state.widget_open = true;
                            }
                            crate::db::worker::Saved::Settings => self.drafts.settings = None,
                            crate::db::worker::Saved::Deletion => self.drafts.delete_entry = None,
                        }
                    }
                    Event::Changed(snapshot) => self.state.data = Some(snapshot),
                    Event::Imported(count) => {
                        self.state.import = ImportState::Done(format!(
                            "Imported {} sessions and {} projects. Entries that an earlier import added were skipped.",
                            count.sessions, count.projects
                        ));
                    }
                    Event::Failed(problem) => {
                        self.state.saving = false;
                        if matches!(self.state.import, ImportState::Running(_)) {
                            self.state.import = ImportState::Failed(problem.clone());
                        }
                        self.state.problem = Some(problem);
                    }
                }
            }
        }
        while let Some(progress) = self.import.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.state.import = match progress {
                Progress::Step(step) => ImportState::Running(step),
                Progress::Sent => ImportState::Running("Saving the imported data".into()),
                Progress::Failed(problem) => ImportState::Failed(problem),
            };
        }
    }

    fn schedule_repaint(&self, ctx: &egui::Context) {
        let running = self
            .state
            .data
            .as_ref()
            .is_some_and(|data| data.running().next().is_some());
        if running {
            ctx.request_repaint_after(Duration::from_secs(1));
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.receive(ui.ctx());
        let clock = ui::Clock {
            now: Timestamp::now(),
            tz: TimeZone::system(),
        };
        for action in ui::shell(ui, &self.state, &mut self.drafts, &clock) {
            self.apply(action, ui.ctx());
        }
        if self.state.widget_open {
            let mut close = false;
            let mut widget_actions = Vec::new();
            let rows = self.state.data.as_ref().map_or(0, |data| data.rows.len());
            ui.ctx().show_viewport_immediate(
                egui::ViewportId::from_hash_of("task-widget"),
                egui::ViewportBuilder::default()
                    .with_title("Trackfast widget")
                    .with_inner_size([350.0, ui::widget_height(rows)])
                    .with_min_inner_size([300.0, 160.0])
                    .with_window_level(egui::WindowLevel::AlwaysOnTop),
                |widget_ui, _class| {
                    close = widget_ui.input(|input| input.viewport().close_requested());
                    egui::CentralPanel::default()
                        .frame(
                            egui::Frame::new()
                                .fill(ui::widget_background())
                                .inner_margin(8),
                        )
                        .show(widget_ui, |ui| {
                            widget_actions = ui::widget(ui, &self.state, &mut self.drafts, &clock);
                        });
                },
            );
            for action in widget_actions {
                self.apply(action, ui.ctx());
            }
            if close {
                self.apply(Action::CloseWidget, ui.ctx());
            }
        }
        self.schedule_repaint(ui.ctx());
    }
}

#[cfg(test)]
mod account_tests {
    use super::*;

    #[test]
    fn signin_moves_the_secret_once_and_cancel_blocks_browser_launch() {
        let (account, commands) = crate::account::Account::for_test();
        let mut app = App {
            account,
            state: State::default(),
            drafts: Drafts::default(),
            database: None,
            import: None,
        };
        app.state.account.status = crate::account::Status::SignedOut;
        *app.drafts.account_secret = "synthetic-desktop-secret".into();
        let ctx = egui::Context::default();
        app.apply(Action::SignIn, &ctx);
        app.apply(Action::SignIn, &ctx);
        assert!(app.drafts.account_secret.is_empty());
        assert!(
            matches!(commands.try_recv().unwrap(),crate::account::Command::SignIn(secret) if secret.as_str()=="synthetic-desktop-secret")
        );
        assert!(commands.try_recv().is_err());
        app.apply(Action::CancelSignIn, &ctx);
        assert!(!app.account.browser_allowed());
    }
}
