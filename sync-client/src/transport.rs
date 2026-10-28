mod retrieval;
use crate::{Queue, Scope};
pub use retrieval::{ChangePage, Page, Project, RemoteChange};
use serde_json::{Value, json};
use std::{net::SocketAddr, time::Duration};

#[derive(Debug)]
pub enum SyncError {
    Unavailable,
    RateLimited { retry_after_seconds: Option<u64> },
    InvalidConfiguration,
    IdentityMismatch,
    Authentication,
    Conflict,
    Rejected,
    InvalidReceipt,
    Storage,
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn receipts_cannot_change_or_omit_recorded_rounding() {
        for receipt in [
            json!({"rounding_interval_minutes":5,"effective_seconds":900,"recorded_settings":{"rounding_interval_minutes":null}}),
            json!({"rounding_interval_minutes":5,"effective_seconds":900,"recorded_settings":null}),
            json!({"rounding_interval_minutes":5,"effective_seconds":900,"recorded_settings":{}}),
        ] {
            assert!(checked_rounding(&receipt, 781).is_err());
        }
    }

    #[test]
    fn a_missing_update_requires_conflict_review_not_an_assumed_deletion() {
        assert!(mutation_conflict(404, "replace"));
        assert!(mutation_conflict(404, "delete"));
        assert!(!mutation_conflict(404, "create"));
        assert!(!mutation_conflict(422, "replace"));
    }

    #[test]
    fn retry_after_headers_support_seconds_and_http_dates() {
        let response = ureq::http::Response::builder()
            .status(429)
            .header("retry-after", "120")
            .body(())
            .unwrap();
        assert!(matches!(
            check_response(&response, 200),
            Err(SyncError::RateLimited {
                retry_after_seconds: Some(120)
            })
        ));
        let now = chrono::DateTime::parse_from_rfc2822("Wed, 21 Oct 2015 07:27:00 GMT")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_eq!(
            retry_after(Some("Wed, 21 Oct 2015 07:28:00 GMT"), now),
            Some(60)
        );
        assert_eq!(
            retry_after(Some("Wed, 21 Oct 2015 07:26:00 GMT"), now),
            Some(0)
        );
        assert_eq!(retry_after(Some("invalid"), now), None);
        assert_eq!(
            retry_after(
                Some("Wed, 21 Oct 2015 07:28:00 GMT"),
                now + chrono::Duration::nanoseconds(999_999_999)
            ),
            Some(60)
        );
        assert_eq!(
            retry_after(
                Some("Wed, 21 Oct 2015 07:28:00 GMT"),
                now + chrono::Duration::milliseconds(500)
            ),
            Some(60)
        );
    }

    #[test]
    fn forbidden_response_does_not_request_token_renewal() {
        assert!(matches!(check_status(403, 200), Err(SyncError::Rejected)));
        assert!(matches!(
            check_status(401, 200),
            Err(SyncError::Authentication)
        ));
    }

    #[test]
    fn receipt_rounding_accepts_exact_and_rounded_totals_and_rejects_false_totals() {
        for (original, interval, effective) in [
            (781, Value::Null, 781),
            (781, json!(5), 900),
            (149, json!(5), 0),
            (150, json!(5), 300),
            (10800, json!(5), 10800),
        ] {
            let receipt =
                json!({"rounding_interval_minutes":interval,"effective_seconds":effective});
            assert_eq!(checked_rounding(&receipt, original).unwrap().0, effective);
            let mut wrong = receipt;
            wrong["effective_seconds"] = json!(effective + 1);
            assert!(checked_rounding(&wrong, original).is_err());
        }
        for receipt in [
            json!({"effective_seconds":781}),
            json!({"rounding_interval_minutes":10,"effective_seconds":600}),
            json!({"rounding_interval_minutes":5,"effective_seconds":-1}),
        ] {
            assert!(checked_rounding(&receipt, 781).is_err());
        }
    }

    #[test]
    fn development_transport_rejects_remote_or_ambiguous_destinations() {
        for server in [
            "http://example.com:8788",
            "https://127.0.0.1:8788",
            "http://user:secret@127.0.0.1:8788",
            "http://127.0.0.1:8788/path",
            "http://127.0.0.1:8788?next=x",
            "http://127.0.0.1:8788#x",
        ] {
            let scope = Scope {
                server: server.into(),
                tenant_id: Uuid::new_v4(),
                person_id: Uuid::new_v4(),
            };
            assert!(
                DevelopmentClient::new(scope, "test-token-with-at-least-32-characters").is_err()
            );
        }
    }
}

/// A blocking client for the loopback development API.
/// Use this client only on a worker thread. Tokens remain in memory.
pub struct DevelopmentClient {
    scope: Scope,
    authorization: String,
    agent: ureq::Agent,
}

#[derive(Debug)]
pub struct RemoteEntry {
    pub entry: crate::DurationEntry,
    pub version: i64,
    pub effective_seconds: u64,
    pub rounding_interval_minutes: Option<u8>,
}

impl DevelopmentClient {
    pub fn read(&self, entry_id: uuid::Uuid) -> Result<RemoteEntry, SyncError> {
        self.check_identity()?;
        let mut response = self
            .agent
            .get(format!("{}/v1/me/entries/{entry_id}", self.scope.server))
            .header("authorization", &self.authorization)
            .call()
            .map_err(|_| SyncError::Unavailable)?;
        check_response(&response, 200)?;
        let etag = response
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
            .ok_or(SyncError::InvalidReceipt)?;
        let value: Value = response
            .body_mut()
            .with_config()
            .limit(32768)
            .read_json()
            .map_err(|_| SyncError::InvalidReceipt)?;
        let entry: crate::DurationEntry =
            crate::Entry::from_response(&value).map_err(|_| SyncError::InvalidReceipt)?;
        crate::validate(&entry).map_err(|_| SyncError::InvalidReceipt)?;
        let version = value["version"]
            .as_i64()
            .filter(|v| *v > 0)
            .ok_or(SyncError::InvalidReceipt)?;
        let (effective_seconds, rounding_interval_minutes) =
            checked_rounding(&value, entry.original_seconds)?;
        if entry.entry_id != entry_id
            || value["person_id"] != json!(self.scope.person_id)
            || etag != format!("\"{version}\"")
        {
            return Err(SyncError::InvalidReceipt);
        }
        Ok(RemoteEntry {
            entry,
            version,
            effective_seconds,
            rounding_interval_minutes,
        })
    }

    pub fn new(scope: Scope, token: &str) -> Result<Self, SyncError> {
        let address = scope
            .server
            .strip_prefix("http://")
            .and_then(|s| s.parse::<SocketAddr>().ok())
            .ok_or(SyncError::InvalidConfiguration)?;
        if !address.ip().is_loopback()
            || address.port() == 0
            || scope.server != format!("http://{address}")
            || token.len() < 32
            || !token.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err(SyncError::InvalidConfiguration);
        }
        let agent = ureq::Agent::config_builder()
            .proxy(None)
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(15)))
            .build()
            .into();
        Ok(Self {
            scope,
            authorization: format!("Bearer {token}"),
            agent,
        })
    }

    pub(crate) fn check_identity(&self) -> Result<(), SyncError> {
        let mut identity = self
            .agent
            .get(format!("{}/v1/me", self.scope.server))
            .header("authorization", &self.authorization)
            .call()
            .map_err(|_| SyncError::Unavailable)?;
        check_response(&identity, 200)?;
        let identity: Value = identity
            .body_mut()
            .with_config()
            .limit(4096)
            .read_json()
            .map_err(|_| SyncError::InvalidReceipt)?;
        if identity != json!({"tenant_id":self.scope.tenant_id,"person_id":self.scope.person_id}) {
            return Err(SyncError::IdentityMismatch);
        }
        Ok(())
    }

    pub fn sync(&self, queue: &mut Queue) -> Result<usize, SyncError> {
        self.sync_batch(queue, usize::MAX)
    }

    /// Sends at most `limit` pending operations. The limit must be positive.
    pub fn sync_batch(&self, queue: &mut Queue, limit: usize) -> Result<usize, SyncError> {
        if limit == 0 {
            return Err(SyncError::InvalidConfiguration);
        }
        if queue.scope != self.scope {
            return Err(SyncError::IdentityMismatch);
        }
        if queue.has_conflicts().map_err(|_| SyncError::Storage)? {
            return Err(SyncError::Conflict);
        }
        self.check_identity()?;
        let mut confirmed = 0;
        for pending in queue
            .pending()
            .map_err(|_| SyncError::Storage)?
            .into_iter()
            .take(limit)
        {
            let entry = &pending.entry;
            let body = entry.request();
            let (action, expected) = queue
                .operation_details(pending.operation_id)
                .map_err(|_| SyncError::Storage)?;
            let resource = format!("{}/v1/me/entries/{}", self.scope.server, entry.entry_id);
            let key = pending.operation_id.to_string();
            let etag = format!("\"{}\"", expected.unwrap_or(0));
            let mut response = match action.as_str() {
                "create" => self
                    .agent
                    .post(format!("{}/v1/me/entries", self.scope.server))
                    .header("authorization", &self.authorization)
                    .header("idempotency-key", &key)
                    .send_json(&body),
                "replace" => {
                    let mut update = body.clone();
                    update.as_object_mut().unwrap().remove("entry_id");
                    update.as_object_mut().unwrap().remove("recorded_settings");
                    self.agent
                        .put(&resource)
                        .header("authorization", &self.authorization)
                        .header("idempotency-key", &key)
                        .header("if-match", &etag)
                        .send_json(&update)
                }
                "delete" => self
                    .agent
                    .delete(&resource)
                    .header("authorization", &self.authorization)
                    .header("idempotency-key", &key)
                    .header("if-match", &etag)
                    .call(),
                _ => return Err(SyncError::Storage),
            }
            .map_err(|_| SyncError::Unavailable)?;
            let status = match action.as_str() {
                "create" => 201,
                "replace" => 200,
                _ => 204,
            };
            if mutation_conflict(response.status().as_u16(), &action) {
                queue
                    .mark_conflict(pending.operation_id)
                    .map_err(|_| SyncError::Storage)?;
                return Err(SyncError::Conflict);
            }
            check_response(&response, status)?;
            if action == "delete" {
                queue
                    .confirm(pending.operation_id)
                    .map_err(|_| SyncError::Storage)?;
                confirmed += 1;
                continue;
            }
            let version = expected
                .unwrap_or(0)
                .checked_add(1)
                .ok_or(SyncError::InvalidReceipt)?;
            let etag = format!("\"{version}\"");
            if response.headers().get("etag").and_then(|v| v.to_str().ok()) != Some(etag.as_str()) {
                return Err(SyncError::InvalidReceipt);
            }
            let receipt: Value = response
                .body_mut()
                .with_config()
                .limit(32768)
                .read_json()
                .map_err(|_| SyncError::InvalidReceipt)?;
            checked_rounding(&receipt, entry.original_seconds)?;
            if let Some(snapshot) = &entry.recorded_settings
                && receipt["rounding_interval_minutes"] != json!(snapshot.rounding_interval_minutes)
            {
                return Err(SyncError::InvalidReceipt);
            }
            if crate::Entry::from_response(&receipt).map_err(|_| SyncError::InvalidReceipt)?
                != *entry
            {
                return Err(SyncError::InvalidReceipt);
            }
            if body
                .as_object()
                .unwrap()
                .iter()
                .any(|(key, value)| receipt.get(key) != Some(value))
                || receipt["person_id"] != json!(self.scope.person_id)
                || receipt["version"] != version
                || receipt["export_state"] != "not_exported"
                || receipt["updated_at"]
                    .as_str()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .is_none()
            {
                return Err(SyncError::InvalidReceipt);
            }
            queue
                .confirm(pending.operation_id)
                .map_err(|_| SyncError::Storage)?;
            confirmed += 1;
        }
        Ok(confirmed)
    }
}

fn checked_rounding(receipt: &Value, original: u64) -> Result<(u64, Option<u8>), SyncError> {
    if let Some(snapshot) = receipt.get("recorded_settings") {
        let fields = snapshot.as_object().ok_or(SyncError::InvalidReceipt)?;
        if fields.len() != 1
            || fields.get("rounding_interval_minutes") != receipt.get("rounding_interval_minutes")
        {
            return Err(SyncError::InvalidReceipt);
        }
    }
    let (effective, interval) = match receipt.get("rounding_interval_minutes") {
        Some(Value::Null) => (original, None),
        Some(value) if value.as_u64() == Some(5) => (((original + 150) / 300) * 300, Some(5)),
        _ => return Err(SyncError::InvalidReceipt),
    };
    if receipt["effective_seconds"].as_u64() != Some(effective) {
        return Err(SyncError::InvalidReceipt);
    }
    Ok((effective, interval))
}

fn mutation_conflict(status: u16, action: &str) -> bool {
    matches!(status, 409 | 412) || (status == 404 && action != "create")
}
fn retry_after(value: Option<&str>, now: chrono::DateTime<chrono::Utc>) -> Option<u64> {
    let value = value?.trim();
    if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) {
        return Some(value.parse().unwrap_or(u64::MAX));
    }
    let date = chrono::DateTime::parse_from_rfc2822(value).ok()?;
    let delay = date.signed_duration_since(now);
    let seconds = delay.num_seconds().max(0);
    Some(seconds as u64 + u64::from(delay > chrono::Duration::seconds(seconds)))
}
fn check_response<T>(response: &ureq::http::Response<T>, expected: u16) -> Result<(), SyncError> {
    if response.status().as_u16() == 429 {
        return Err(SyncError::RateLimited {
            retry_after_seconds: retry_after(
                response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok()),
                chrono::Utc::now(),
            ),
        });
    }
    check_status(response.status().as_u16(), expected)
}
fn check_status(status: u16, expected: u16) -> Result<(), SyncError> {
    if status == expected {
        return Ok(());
    }
    Err(match status {
        401 => SyncError::Authentication,
        409 | 412 => SyncError::Conflict,
        400 | 403 | 404 | 422 | 428 => SyncError::Rejected,
        429 => SyncError::RateLimited {
            retry_after_seconds: None,
        },
        500..=599 => SyncError::Unavailable,
        _ => SyncError::InvalidReceipt,
    })
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Unavailable => "The API is unavailable. Local work remains pending.",
            Self::RateLimited { .. } => {
                "The API rate limit requires a delay. Local work remains pending."
            }
            Self::InvalidConfiguration => "Check the local API address and development token.",
            Self::IdentityMismatch => "The API account does not match this queue.",
            Self::Authentication => "The API did not accept the credentials.",
            Self::Conflict => "The server entry conflicts with local work.",
            Self::Rejected => "The API rejected the operation. Local work remains pending.",
            Self::InvalidReceipt => "The API response did not confirm this operation.",
            Self::Storage => "The local queue operation failed.",
        })
    }
}
impl std::error::Error for SyncError {}
