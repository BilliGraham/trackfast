//! Local time-tracking data that the views read.

pub mod summary;
pub mod week;

use jiff::{SignedDuration, Timestamp};

/// The widget holds at most this many task rows.
pub const MAX_ROWS: usize = 3;

/// 0 is Sunday, 1 is Monday. A setting can replace this later.
pub const BEGINNING_OF_WEEK: u8 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub id: i64,
    pub description: String,
    pub project_id: Option<i64>,
    pub start: Timestamp,
    /// `None` while the session runs.
    pub stop: Option<Timestamp>,
}

impl Session {
    pub fn is_running(&self) -> bool {
        self.stop.is_none()
    }

    pub fn elapsed(&self, now: Timestamp) -> SignedDuration {
        self.stop
            .unwrap_or(now)
            .duration_since(self.start)
            .max(SignedDuration::ZERO)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    pub id: i64,
    pub name: String,
    /// `#rrggbb`.
    pub colour: String,
    pub client_id: Option<i64>,
    pub archived: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Client {
    pub id: i64,
    pub name: String,
}

/// A description and project pair that the widget can continue.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Task {
    pub description: String,
    pub project_id: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WidgetRow {
    pub id: i64,
    pub task: Task,
    /// The running session, if the row runs.
    pub session: Option<Session>,
}

/// Everything the views need, loaded in one read.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub projects: Vec<Project>,
    pub clients: Vec<Client>,
    pub rows: Vec<WidgetRow>,
    /// Sessions that started at or after `loaded_from`, plus all running sessions.
    pub sessions: Vec<Session>,
    pub loaded_from: Timestamp,
    /// Recent distinct tasks, newest first.
    pub recent_tasks: Vec<Task>,
}

impl Snapshot {
    pub fn project(&self, id: i64) -> Option<&Project> {
        self.projects.iter().find(|project| project.id == id)
    }

    pub fn client(&self, id: i64) -> Option<&Client> {
        self.clients.iter().find(|client| client.id == id)
    }

    pub fn running(&self) -> impl Iterator<Item = &Session> {
        self.sessions.iter().filter(|session| session.is_running())
    }

    pub fn has_room(&self) -> bool {
        self.rows.len() < MAX_ROWS
    }

    /// Today's total for a task, from sessions that started at or after `since`.
    pub fn task_total(&self, task: &Task, since: Timestamp, now: Timestamp) -> SignedDuration {
        self.sessions
            .iter()
            .filter(|session| {
                session.start >= since
                    && session.description == task.description
                    && session.project_id == task.project_id
            })
            .fold(SignedDuration::ZERO, |total, session| {
                total + session.elapsed(now)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(start: &str, stop: Option<&str>) -> Session {
        Session {
            id: 1,
            description: "Review".into(),
            project_id: Some(3),
            start: start.parse().unwrap(),
            stop: stop.map(|stop| stop.parse().unwrap()),
        }
    }

    #[test]
    fn running_sessions_count_until_now() {
        let now = "2026-10-16T10:00:42Z".parse().unwrap();
        assert_eq!(
            session("2026-10-16T10:00:00Z", None).elapsed(now).as_secs(),
            42
        );
        assert_eq!(
            session("2026-10-16T08:00:00Z", Some("2026-10-16T09:30:00Z"))
                .elapsed(now)
                .as_secs(),
            5400
        );
    }

    #[test]
    fn task_total_matches_description_and_project() {
        let mut other = session("2026-10-16T09:00:00Z", Some("2026-10-16T09:10:00Z"));
        other.project_id = None;
        let snapshot = Snapshot {
            sessions: vec![
                session("2026-10-16T08:00:00Z", Some("2026-10-16T08:30:00Z")),
                session("2026-10-16T10:00:00Z", None),
                session("2026-10-15T08:00:00Z", Some("2026-10-15T09:00:00Z")),
                other,
            ],
            ..Default::default()
        };
        let task = Task {
            description: "Review".into(),
            project_id: Some(3),
        };
        let total = snapshot.task_total(
            &task,
            "2026-10-16T00:00:00Z".parse().unwrap(),
            "2026-10-16T10:15:00Z".parse().unwrap(),
        );
        assert_eq!(total.as_secs(), 45 * 60);
    }
}
