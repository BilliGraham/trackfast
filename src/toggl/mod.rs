mod client;
pub mod credentials;
pub(crate) mod quota;
pub mod summary;

use std::fmt;

use jiff::{SignedDuration, Timestamp};
use serde::Deserialize;

pub use client::{ApiError, Client};

/// A Toggl Track API token. Its `Debug` output never shows the value.
pub struct ApiToken(String);

impl ApiToken {
    /// Returns `None` when the input is empty after trimming.
    pub fn new(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        (!trimmed.is_empty()).then(|| Self(trimmed.to_owned()))
    }

    fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiToken(<redacted>)")
    }
}

#[derive(Clone, Debug)]
pub struct User {
    pub fullname: Option<String>,
    /// 0 is Sunday, 1 is Monday, and so on.
    pub beginning_of_week: u8,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Workspace {
    pub id: i64,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Project {
    pub id: i64,
    pub workspace_id: i64,
    pub name: String,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default = "active_by_default")]
    pub active: bool,
}

fn active_by_default() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize)]
pub struct TimeEntry {
    #[serde(default)]
    pub project_id: Option<i64>,
    #[serde(default)]
    pub description: Option<String>,
    pub start: Timestamp,
    #[serde(default)]
    pub stop: Option<Timestamp>,
    /// Seconds. Toggl sends a negative value while the entry runs.
    pub duration: i64,
}

impl TimeEntry {
    pub fn is_running(&self) -> bool {
        self.duration < 0
    }

    pub fn elapsed(&self, now: Timestamp) -> SignedDuration {
        if self.is_running() {
            now.duration_since(self.start).max(SignedDuration::ZERO)
        } else {
            SignedDuration::from_secs(self.duration)
        }
    }
}

/// The read-only Toggl data that one sync loads.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub user: User,
    pub workspaces: Vec<Workspace>,
    pub projects: Vec<Project>,
    pub entries: Vec<TimeEntry>,
    pub fetched_at: Timestamp,
}

impl Snapshot {
    pub fn project(&self, id: i64) -> Option<&Project> {
        self.projects.iter().find(|project| project.id == id)
    }

    pub fn running(&self) -> Option<&TimeEntry> {
        self.entries.iter().find(|entry| entry.is_running())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_debug_hides_the_value() {
        let token = ApiToken::new("  secret-value \n").unwrap();
        assert_eq!(token.expose(), "secret-value");
        assert!(!format!("{token:?}").contains("secret"));
    }

    #[test]
    fn blank_token_is_rejected() {
        assert!(ApiToken::new(" \t ").is_none());
    }

    #[test]
    fn decodes_documented_time_entry_fields() {
        let json = r#"[
            {"id": 1, "workspace_id": 7, "project_id": null, "description": null,
             "start": "2026-10-12T08:00:00+00:00", "stop": "2026-10-12T09:30:00+00:00",
             "duration": 5400, "tags": null, "billable": false},
            {"id": 2, "workspace_id": 7, "project_id": 3, "description": "Review",
             "start": "2026-10-12T10:00:00Z", "stop": null, "duration": -1}
        ]"#;
        let entries: Vec<TimeEntry> = serde_json::from_str(json).unwrap();
        assert!(!entries[0].is_running());
        assert_eq!(entries[0].elapsed(Timestamp::UNIX_EPOCH).as_secs(), 5400);
        assert!(entries[1].is_running());
        let now: Timestamp = "2026-10-12T10:00:42Z".parse().unwrap();
        assert_eq!(entries[1].elapsed(now).as_secs(), 42);
    }

    #[test]
    fn duration_only_entries_keep_their_completed_duration() {
        let entry: TimeEntry =
            serde_json::from_str(r#"{"start":"2026-10-16T08:00:00Z","stop":null,"duration":3600}"#)
                .unwrap();
        let now = "2026-10-16T13:00:00Z".parse().unwrap();
        assert!(!entry.is_running());
        assert_eq!(entry.elapsed(now).as_secs(), 3600);
    }
}
