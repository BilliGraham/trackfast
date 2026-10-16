//! Copies Toggl history into the local database on request.

use std::sync::mpsc::{self, Receiver};
use std::thread;

use eframe::egui;
use jiff::{SignedDuration, Timestamp};

use crate::db::worker::{Command, Database};
use crate::db::{Import, ImportedProject, ImportedSession};
use crate::toggl::quota::RequestQuota;
use crate::toggl::{ApiToken, Client, Me, TimeEntry};

pub const DEFAULT_COLOUR: &str = "#9e9e9e";
/// Each entries request covers at most this span, so a long import stays
/// within any per-request range limit.
const WINDOW: SignedDuration = SignedDuration::from_hours(30 * 24);

pub enum Progress {
    Step(String),
    /// The data went to the database worker, which reports the counts.
    Sent,
    Failed(String),
}

pub fn start(
    token: ApiToken,
    from: Timestamp,
    database: Database,
    ctx: egui::Context,
) -> Receiver<Progress> {
    let (progress, receiver) = mpsc::channel();
    thread::Builder::new()
        .name("toggl-import".into())
        .spawn(move || {
            let notify = |event| {
                if progress.send(event).is_ok() {
                    ctx.request_repaint();
                }
            };
            match fetch(&token, from, Timestamp::now(), &notify) {
                Ok(data) => {
                    database.send(Command::Import(data));
                    notify(Progress::Sent);
                }
                Err(error) => notify(Progress::Failed(error)),
            }
        })
        .expect("the operating system must allow a new thread");
    receiver
}

fn fetch(
    token: &ApiToken,
    from: Timestamp,
    until: Timestamp,
    notify: &impl Fn(Progress),
) -> Result<Import, String> {
    let client = Client::new(token);
    let mut quota = RequestQuota::default();
    notify(Progress::Step("Loading projects and clients".into()));
    let me = client
        .fetch_me(&mut quota)
        .map_err(|error| error.to_string())?;
    let mut entries = Vec::new();
    let mut window_start = from;
    while window_start < until {
        let window_end = (window_start + WINDOW).min(until);
        notify(Progress::Step(format!(
            "Loading entries from {}",
            window_start.strftime("%-d %b %Y")
        )));
        entries.extend(
            client
                .fetch_entries(window_start, window_end, &mut quota)
                .map_err(|error| error.to_string())?,
        );
        window_start = window_end;
    }
    Ok(map(me, entries))
}

pub fn map(me: Me, entries: Vec<TimeEntry>) -> Import {
    Import {
        clients: me
            .clients
            .unwrap_or_default()
            .into_iter()
            .map(|client| (client.id, client.name))
            .collect(),
        projects: me
            .projects
            .unwrap_or_default()
            .into_iter()
            .map(|project| ImportedProject {
                toggl_id: project.id,
                name: project.name,
                colour: project
                    .color
                    .filter(|colour| colour.len() == 7 && colour.starts_with('#'))
                    .unwrap_or_else(|| DEFAULT_COLOUR.into()),
                client_toggl_id: project.client_id,
                archived: !project.active,
            })
            .collect(),
        sessions: entries
            .into_iter()
            .filter_map(|entry| {
                let stop = entry.completed_stop()?;
                Some(ImportedSession {
                    toggl_id: entry.id,
                    description: entry.description.unwrap_or_default(),
                    project_toggl_id: entry.project_id,
                    start: entry.start,
                    stop,
                })
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_maps_projects_clients_and_completed_entries() {
        let me: Me = serde_json::from_str(
            r##"{"clients": [{"id": 5, "name": "Acme"}],
                 "projects": [
                    {"id": 3, "name": "Site", "client_id": 5, "color": "#06aaf5", "active": false},
                    {"id": 4, "name": "Plain", "color": null}
                 ]}"##,
        )
        .unwrap();
        let entries: Vec<TimeEntry> = serde_json::from_str(
            r#"[
                {"id": 1, "project_id": 3, "description": "Review",
                 "start": "2026-10-12T08:00:00Z", "stop": "2026-10-12T09:00:00Z", "duration": 3600},
                {"id": 2, "description": null,
                 "start": "2026-10-12T10:00:00Z", "stop": null, "duration": -1}
            ]"#,
        )
        .unwrap();
        let data = map(me, entries);
        assert_eq!(data.clients, [(5, "Acme".to_owned())]);
        assert!(data.projects[0].archived);
        assert_eq!(data.projects[0].client_toggl_id, Some(5));
        assert_eq!(data.projects[1].colour, DEFAULT_COLOUR);
        assert_eq!(data.sessions.len(), 1);
        assert_eq!(data.sessions[0].project_toggl_id, Some(3));
    }
}
