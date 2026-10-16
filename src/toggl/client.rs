use std::fmt;
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::quota::RequestQuota;
use super::{ApiToken, TimeEntry, TogglClient, TogglProject};

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

/// The parts of `GET /me?with_related_data=true` that the import uses.
#[derive(Debug, Default, Deserialize)]
pub struct Me {
    #[serde(default)]
    pub clients: Option<Vec<TogglClient>>,
    #[serde(default)]
    pub projects: Option<Vec<TogglProject>>,
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

    /// Loads the clients and projects. This uses one API request.
    pub fn fetch_me(&self, quota: &mut RequestQuota) -> Result<Me, ApiError> {
        self.get("/me", &[("with_related_data", "true")], quota)
    }

    /// Loads the entries that started from `start` up to `end`. This uses one API request.
    pub fn fetch_entries(
        &self,
        start: Timestamp,
        end: Timestamp,
        quota: &mut RequestQuota,
    ) -> Result<Vec<TimeEntry>, ApiError> {
        self.get(
            "/me/time_entries",
            &[("start_date", &rfc3339(start)), ("end_date", &rfc3339(end))],
            quota,
        )
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
    fn decodes_me_with_related_data() {
        let json = r##"{
            "id": 9, "email": "a@example.com", "fullname": "Ada", "beginning_of_week": 1,
            "clients": [{"id": 5, "name": "Acme", "wid": 7}],
            "projects": [{"id": 3, "workspace_id": 7, "client_id": 5, "name": "Site",
                          "color": "#06aaf5", "active": false}],
            "time_entries": [], "tags": null
        }"##;
        let me: Me = serde_json::from_str(json).unwrap();
        assert_eq!(me.clients.unwrap()[0].name, "Acme");
        let project = &me.projects.unwrap()[0];
        assert_eq!(project.client_id, Some(5));
        assert!(!project.active);
    }
}
