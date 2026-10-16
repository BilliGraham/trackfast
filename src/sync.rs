//! Runs all Toggl requests and credential-store calls on one background thread.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Instant;

use eframe::egui;
use jiff::Timestamp;
use jiff::tz::TimeZone;

use crate::toggl::quota::RequestQuota;
use crate::toggl::{ApiError, ApiToken, Client, Snapshot, TimeEntry, credentials};

pub enum Command {
    SignIn(ApiToken),
    Refresh,
    /// Loads the entries that started from `from` up to `to`.
    LoadEarlier {
        from: Timestamp,
        to: Timestamp,
    },
    SignOut,
}

pub enum Event {
    SignedOut {
        problem: Option<String>,
    },
    Busy,
    RequestLimit(Option<Instant>),
    Loaded {
        snapshot: Snapshot,
        problem: Option<String>,
    },
    EarlierLoaded {
        from: Timestamp,
        to: Timestamp,
        entries: Vec<TimeEntry>,
    },
    Failed {
        problem: String,
        signed_in: bool,
    },
}

pub struct Sync {
    commands: Sender<Command>,
    events: Receiver<Event>,
}

impl Sync {
    pub fn start(ctx: egui::Context) -> Self {
        let (commands, command_rx) = mpsc::channel();
        let (event_tx, events) = mpsc::channel();
        thread::Builder::new()
            .name("toggl-sync".into())
            .spawn(move || {
                Worker {
                    events: event_tx,
                    ctx,
                    backend: SystemBackend,
                    client: None,
                    quota: RequestQuota::default(),
                }
                .run(command_rx)
            })
            .expect("the operating system must allow a new thread");
        Self { commands, events }
    }

    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    pub fn try_recv(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }
}

trait Backend {
    fn load_token(&mut self) -> Result<Option<ApiToken>, String>;
    fn save_token(&mut self, token: &ApiToken) -> Result<(), String>;
    fn delete_token(&mut self) -> Result<(), String>;
    fn fetch(&mut self, client: &Client, quota: &mut RequestQuota) -> Result<Snapshot, ApiError>;
    fn fetch_entries(
        &mut self,
        client: &Client,
        from: Timestamp,
        to: Timestamp,
        quota: &mut RequestQuota,
    ) -> Result<Vec<TimeEntry>, ApiError>;
}

struct SystemBackend;

impl Backend for SystemBackend {
    fn load_token(&mut self) -> Result<Option<ApiToken>, String> {
        credentials::load().map_err(|error| error.to_string())
    }

    fn save_token(&mut self, token: &ApiToken) -> Result<(), String> {
        credentials::save(token).map_err(|error| error.to_string())
    }

    fn delete_token(&mut self) -> Result<(), String> {
        credentials::delete().map_err(|error| error.to_string())
    }

    fn fetch(&mut self, client: &Client, quota: &mut RequestQuota) -> Result<Snapshot, ApiError> {
        client.fetch(&TimeZone::system(), quota)
    }

    fn fetch_entries(
        &mut self,
        client: &Client,
        from: Timestamp,
        to: Timestamp,
        quota: &mut RequestQuota,
    ) -> Result<Vec<TimeEntry>, ApiError> {
        client.fetch_entries(from, to, quota)
    }
}

struct Worker<B> {
    events: Sender<Event>,
    ctx: egui::Context,
    backend: B,
    client: Option<Client>,
    quota: RequestQuota,
}

impl<B: Backend> Worker<B> {
    fn run(mut self, commands: Receiver<Command>) {
        self.restore();
        while let Ok(command) = commands.recv() {
            self.handle(command);
        }
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::SignIn(token) => self.load(Some(token)),
            Command::Refresh => self.load(None),
            Command::LoadEarlier { from, to } => self.load_earlier(from, to),
            Command::SignOut => {
                self.client = None;
                let problem = self
                    .backend
                    .delete_token()
                    .err()
                    .map(|error| format!("Trackfast cannot remove the saved token: {error}"));
                self.notify(Event::SignedOut { problem });
            }
        }
    }

    fn restore(&mut self) {
        match self.backend.load_token() {
            Ok(Some(token)) => {
                self.client = Some(Client::new(&token));
                self.load(None);
            }
            Ok(None) => self.notify(Event::SignedOut { problem: None }),
            Err(error) => self.notify(Event::SignedOut {
                problem: Some(format!(
                    "Trackfast cannot read the credential store: {error}"
                )),
            }),
        }
    }

    fn load(&mut self, token: Option<ApiToken>) {
        let candidate = match &token {
            Some(token) => Some(Client::new(token)),
            None => self.client.take(),
        };
        let Some(candidate) = candidate else {
            self.notify(Event::SignedOut { problem: None });
            return;
        };
        if let Err(retry_at) = self.quota.begin_sync(Instant::now()) {
            if token.is_none() {
                self.client = Some(candidate);
            }
            self.notify(Event::RequestLimit(Some(retry_at)));
            self.notify(Event::Failed {
                problem: ApiError::RateLimited(retry_at).to_string(),
                signed_in: self.client.is_some(),
            });
            return;
        }
        self.notify(Event::Busy);
        let result = self.backend.fetch(&candidate, &mut self.quota);
        if matches!(result, Err(ApiError::QuotaReached)) {
            self.quota.block_after_quota_error(Instant::now());
        }
        self.notify(Event::RequestLimit(self.quota.retry_at(Instant::now())));
        match result {
            Ok(snapshot) => {
                let problem = token.as_ref().and_then(|token| {
                    self.backend.save_token(token).err().map(|error| {
                        format!("Trackfast cannot save the token, so you must sign in again next time: {error}")
                    })
                });
                self.client = Some(candidate);
                self.notify(Event::Loaded { snapshot, problem });
            }
            Err(ApiError::InvalidToken) => {
                self.client = None;
                let mut problem = ApiError::InvalidToken.to_string();
                if token.is_none()
                    && let Err(error) = self.backend.delete_token()
                {
                    problem.push_str(&format!(
                        " Trackfast cannot remove the saved token: {error}"
                    ));
                }
                self.notify(Event::SignedOut {
                    problem: Some(problem),
                });
            }
            Err(error) => {
                if token.is_none() {
                    self.client = Some(candidate);
                }
                self.notify(Event::Failed {
                    problem: error.to_string(),
                    signed_in: self.client.is_some(),
                });
            }
        }
    }

    fn load_earlier(&mut self, from: Timestamp, to: Timestamp) {
        let Some(client) = self.client.take() else {
            return;
        };
        self.notify(Event::Busy);
        let result = self
            .backend
            .fetch_entries(&client, from, to, &mut self.quota);
        if matches!(result, Err(ApiError::QuotaReached)) {
            self.quota.block_after_quota_error(Instant::now());
        }
        self.notify(Event::RequestLimit(self.quota.retry_at(Instant::now())));
        match result {
            Ok(entries) => {
                self.client = Some(client);
                self.notify(Event::EarlierLoaded { from, to, entries });
            }
            Err(ApiError::InvalidToken) => {
                let mut problem = ApiError::InvalidToken.to_string();
                if let Err(error) = self.backend.delete_token() {
                    problem.push_str(&format!(
                        " Trackfast cannot remove the saved token: {error}"
                    ));
                }
                self.notify(Event::SignedOut {
                    problem: Some(problem),
                });
            }
            Err(error) => {
                self.client = Some(client);
                self.notify(Event::Failed {
                    problem: error.to_string(),
                    signed_in: true,
                });
            }
        }
    }

    fn notify(&self, event: Event) {
        if self.events.send(event).is_ok() {
            self.ctx.request_repaint();
        }
    }
}

#[cfg(test)]
mod tests;
