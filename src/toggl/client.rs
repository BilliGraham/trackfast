use std::fmt;
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp, ToSpan};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::quota::RequestQuota;
use super::{ApiToken, Project, Snapshot, TimeEntry, User, Workspace, summary};

const BASE_URL: &str = "https://api.track.toggl.com/api/v9";
const TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug)]
pub enum ApiError {
    InvalidToken,
    /// Toggl answers HTTP 402 when the hourly request quota is used up.
    QuotaReached,
    RateLimited(Instant),
    Status(u16),
    Decode,
    Network(String),
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidToken => {
                f.write_str("Toggl did not accept the API token. Check the token and try again.")
            }
            Self::QuotaReached => f.write_str(
                "The Toggl API request limit for this hour is reached. Try again later.",
            ),
            Self::RateLimited(retry_at) => write!(
                f,
                "Try again in {} seconds.",
                retry_at.saturating_duration_since(Instant::now()).as_secs() + 1
            ),
            Self::Status(code) => write!(f, "Toggl returned HTTP status {code}."),
            Self::Decode => f.write_str("Trackfast cannot read the Toggl response."),
            Self::Network(detail) => write!(f, "Trackfast cannot connect to Toggl: {detail}"),
        }
    }
}

impl From<ureq::Error> for ApiError {
    fn from(error: ureq::Error) -> Self {
        match error {
            ureq::Error::StatusCode(code) => from_status(code),
            ureq::Error::Json(_) => Self::Decode,
            other => Self::Network(other.to_string()),
        }
    }
}

fn from_status(code: u16) -> ApiError {
    match code {
        401 | 403 => ApiError::InvalidToken,
        402 | 429 => ApiError::QuotaReached,
        other => ApiError::Status(other),
    }
}

/// A blocking Toggl Track API v9 client. Call it only from a background thread.
pub struct Client {
    agent: ureq::Agent,
    authorization: String,
}

#[derive(Deserialize)]
struct Me {
    #[serde(default)]
    fullname: Option<String>,
    #[serde(default)]
    beginning_of_week: u8,
    #[serde(default)]
    workspaces: Option<Vec<Workspace>>,
    #[serde(default)]
    projects: Option<Vec<Project>>,
}

impl Client {
    pub fn new(token: &ApiToken) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            .https_only(true)
            .max_redirects(0)
            .http_status_as_error(false)
            .user_agent(concat!("trackfast/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        let credentials = STANDARD.encode(format!("{}:api_token", token.expose()));
        Self {
            agent,
            authorization: format!("Basic {credentials}"),
        }
    }

    /// Loads the user, workspaces, projects, and the time entries from the
    /// start of the previous week. This uses two API requests.
    pub fn fetch(&self, tz: &TimeZone, quota: &mut RequestQuota) -> Result<Snapshot, ApiError> {
        let me: Me = self.get("/me", &[("with_related_data", "true")], quota)?;
        let now = Timestamp::now();
        let range_start = previous_week_start(now, tz, me.beginning_of_week);
        let range_end = now + SignedDuration::from_hours(24);
        let mut entries: Vec<TimeEntry> = self.get(
            "/me/time_entries",
            &[
                ("start_date", &rfc3339(range_start)),
                ("end_date", &rfc3339(range_end)),
            ],
            quota,
        )?;
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.start));
        Ok(Snapshot {
            user: User {
                fullname: me.fullname,
                beginning_of_week: me.beginning_of_week,
            },
            workspaces: me.workspaces.unwrap_or_default(),
            projects: me.projects.unwrap_or_default(),
            entries,
            fetched_at: now,
        })
    }

    fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
        quota: &mut RequestQuota,
    ) -> Result<T, ApiError> {
        quota
            .record_request(Instant::now())
            .map_err(ApiError::RateLimited)?;
        let mut response = self
            .agent
            .get(format!("{BASE_URL}{path}"))
            .header("Authorization", &self.authorization)
            .query_pairs(query.iter().copied())
            .call()?;
        if !response.status().is_success() {
            return Err(from_status(response.status().as_u16()));
        }
        Ok(response.body_mut().read_json()?)
    }
}

fn previous_week_start(now: Timestamp, tz: &TimeZone, beginning_of_week: u8) -> Timestamp {
    let today = now.to_zoned(tz.clone()).date();
    let this_week = summary::week_start(today, beginning_of_week);
    this_week
        .checked_sub(7.days())
        .ok()
        .and_then(|date| summary::start_of_day(date, tz))
        .unwrap_or(now - SignedDuration::from_hours(14 * 24))
}

/// Toggl accepts RFC 3339 times. Whole seconds keep the query simple.
fn rfc3339(timestamp: Timestamp) -> String {
    timestamp.strftime("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_codes_map_to_actionable_errors() {
        assert!(matches!(from_status(401), ApiError::InvalidToken));
        assert!(matches!(from_status(403), ApiError::InvalidToken));
        assert!(matches!(from_status(402), ApiError::QuotaReached));
        assert!(matches!(from_status(429), ApiError::QuotaReached));
        assert!(matches!(from_status(500), ApiError::Status(500)));
    }

    #[test]
    fn query_times_use_utc_whole_seconds() {
        let time: Timestamp = "2026-10-12T08:00:00.123+02:00".parse().unwrap();
        assert_eq!(rfc3339(time), "2026-10-12T06:00:00Z");
    }

    #[test]
    fn range_starts_at_local_midnight_of_previous_week() {
        let tz = TimeZone::fixed(jiff::tz::offset(2));
        // Friday 16 October 2026, 10:00 local time.
        let now: Timestamp = "2026-10-16T08:00:00Z".parse().unwrap();
        let start = previous_week_start(now, &tz, 1);
        assert_eq!(rfc3339(start), "2026-10-04T22:00:00Z");
    }

    #[test]
    fn decodes_me_with_related_data() {
        let json = r##"{
            "id": 9, "email": "a@example.com", "fullname": "Ada", "beginning_of_week": 1,
            "workspaces": [{"id": 7, "name": "Home"}],
            "projects": [{"id": 3, "workspace_id": 7, "name": "Site", "color": "#06aaf5", "active": false}],
            "time_entries": [], "tags": null, "clients": null
        }"##;
        let me: Me = serde_json::from_str(json).unwrap();
        assert_eq!(me.fullname.as_deref(), Some("Ada"));
        assert_eq!(me.workspaces.unwrap()[0].name, "Home");
        assert!(!me.projects.unwrap()[0].active);
    }
}
