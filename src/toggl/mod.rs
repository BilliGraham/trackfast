//! Read-only access to the Toggl Track API v9, used only to import history.

mod client;
pub(crate) mod quota;

use std::fmt;

use jiff::Timestamp;
use serde::Deserialize;

pub use client::{Client, Me};

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

#[derive(Clone, Debug, Deserialize)]
pub struct TogglClient {
    pub id: i64,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TogglProject {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub client_id: Option<i64>,
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
    pub id: i64,
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
    /// The stop time of a completed entry. Running entries have none.
    pub fn completed_stop(&self) -> Option<Timestamp> {
        if self.duration < 0 {
            return None;
        }
        self.stop.or_else(|| {
            self.start
                .checked_add(jiff::SignedDuration::from_secs(self.duration))
                .ok()
        })
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
    fn completed_entries_have_a_stop_and_running_entries_do_not() {
        let json = r#"[
            {"id": 1, "workspace_id": 7, "project_id": null, "description": null,
             "start": "2026-10-12T08:00:00+00:00", "stop": "2026-10-12T09:30:00+00:00",
             "duration": 5400, "tags": null, "billable": false},
            {"id": 2, "workspace_id": 7, "project_id": 3, "description": "Review",
             "start": "2026-10-12T10:00:00Z", "stop": null, "duration": -1},
            {"id": 3, "start": "2026-10-16T08:00:00Z", "stop": null, "duration": 3600}
        ]"#;
        let entries: Vec<TimeEntry> = serde_json::from_str(json).unwrap();
        assert_eq!(
            entries[0].completed_stop(),
            Some("2026-10-12T09:30:00Z".parse().unwrap())
        );
        assert_eq!(entries[1].completed_stop(), None);
        assert_eq!(
            entries[2].completed_stop(),
            Some("2026-10-16T09:00:00Z".parse().unwrap())
        );
    }
}
