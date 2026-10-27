use super::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RemoteChange {
    pub entry_id: Uuid,
    pub version: i64,
    pub entry: Option<crate::Entry>,
}

#[derive(Debug)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct Project {
    pub project_id: Uuid,
    pub name: String,
    pub colour: String,
    pub archived: bool,
}

impl DevelopmentClient {
    fn get_page(&self, path: &str) -> Result<Value, SyncError> {
        self.check_identity()?;
        let mut response = self
            .agent
            .get(format!("{}{path}", self.scope.server))
            .header("authorization", &self.authorization)
            .call()
            .map_err(|_| SyncError::Unavailable)?;
        check_response(&response, 200)?;
        response
            .body_mut()
            .with_config()
            .limit(2_000_000)
            .read_json()
            .map_err(|_| SyncError::InvalidReceipt)
    }

    fn remote_entry(&self, value: &Value) -> Result<RemoteEntry, SyncError> {
        let entry = crate::Entry::from_response(value).map_err(|_| SyncError::InvalidReceipt)?;
        let version = value["version"]
            .as_i64()
            .filter(|v| *v > 0)
            .ok_or(SyncError::InvalidReceipt)?;
        if value["person_id"] != json!(self.scope.person_id)
            || value["export_state"] != "not_exported"
            || value["updated_at"]
                .as_str()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .is_none()
        {
            return Err(SyncError::InvalidReceipt);
        }
        let (effective_seconds, rounding_interval_minutes) =
            checked_rounding(value, entry.original_seconds)?;
        Ok(RemoteEntry {
            entry,
            version,
            effective_seconds,
            rounding_interval_minutes,
        })
    }

    pub fn list_entries(
        &self,
        from: chrono::NaiveDate,
        to: chrono::NaiveDate,
        limit: u16,
        cursor: Option<Uuid>,
    ) -> Result<Page<RemoteEntry>, SyncError> {
        if from > to || !(1..=100).contains(&limit) {
            return Err(SyncError::InvalidConfiguration);
        }
        let value = self.get_page(&format!(
            "/v1/me/entries?from_date={from}&to_date={to}&limit={limit}{}",
            cursor_query(cursor)
        ))?;
        let (items, next_cursor) = page(&value, limit)?;
        let items = items
            .iter()
            .map(|v| self.remote_entry(v))
            .collect::<Result<Vec<_>, _>>()?;
        if items
            .iter()
            .any(|r| r.entry.entry_date < from || r.entry.entry_date > to)
        {
            return Err(SyncError::InvalidReceipt);
        }
        Ok(Page { items, next_cursor })
    }

    pub fn list_projects(
        &self,
        limit: u16,
        cursor: Option<Uuid>,
    ) -> Result<Page<Project>, SyncError> {
        if !(1..=100).contains(&limit) {
            return Err(SyncError::InvalidConfiguration);
        }
        let value = self.get_page(&format!(
            "/v1/projects?limit={limit}{}",
            cursor_query(cursor)
        ))?;
        let (items, next_cursor) = page(&value, limit)?;
        let items = items
            .iter()
            .map(|v| serde_json::from_value(v.clone()).map_err(|_| SyncError::InvalidReceipt))
            .collect::<Result<Vec<Project>, _>>()?;
        if items.iter().any(|p| {
            p.name.trim().is_empty()
                || p.colour.len() != 7
                || !p.colour.starts_with('#')
                || !p.colour.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
        }) {
            return Err(SyncError::InvalidReceipt);
        }
        Ok(Page { items, next_cursor })
    }

    /// Retrieves one bounded page. Returns true when another page is available.
    /// Call again after each page, and later to poll for further changes.
    pub fn pull_changes(&self, queue: &mut Queue) -> Result<bool, SyncError> {
        if queue.scope != self.scope {
            return Err(SyncError::IdentityMismatch);
        }
        let previous = queue.checkpoint().map_err(|_| SyncError::Storage)?;
        let value = self.get_page(&format!(
            "/v1/me/entry-changes?limit=100{}",
            cursor_query(previous)
        ))?;
        let (items, next) = page(&value, 100)?;
        let next = next.ok_or(SyncError::InvalidReceipt)?;
        let more = value["has_more"]
            .as_bool()
            .ok_or(SyncError::InvalidReceipt)?;
        if (more && items.is_empty()) || (!items.is_empty() && Some(next) == previous) {
            return Err(SyncError::InvalidReceipt);
        }
        let changes = items
            .iter()
            .map(|v| {
                let entry_id: Uuid = serde_json::from_value(v["entry_id"].clone())
                    .map_err(|_| SyncError::InvalidReceipt)?;
                let version = v["version"]
                    .as_i64()
                    .filter(|v| *v > 0)
                    .ok_or(SyncError::InvalidReceipt)?;
                let deleted = v["deleted"].as_bool().ok_or(SyncError::InvalidReceipt)?;
                let entry = if deleted {
                    if v.get("entry") != Some(&Value::Null) {
                        return Err(SyncError::InvalidReceipt);
                    }
                    None
                } else {
                    let remote = self.remote_entry(&v["entry"])?;
                    if remote.version != version || remote.entry.entry_id != entry_id {
                        return Err(SyncError::InvalidReceipt);
                    }
                    Some(remote.entry)
                };
                Ok(RemoteChange {
                    entry_id,
                    version,
                    entry,
                })
            })
            .collect::<Result<Vec<_>, SyncError>>()?;
        queue
            .apply_remote_page(previous, next, &changes)
            .map_err(|_| SyncError::Storage)?;
        Ok(more)
    }
}

fn cursor_query(cursor: Option<Uuid>) -> String {
    cursor.map(|v| format!("&cursor={v}")).unwrap_or_default()
}
fn page(value: &Value, limit: u16) -> Result<(&Vec<Value>, Option<Uuid>), SyncError> {
    let items = value["items"]
        .as_array()
        .filter(|v| v.len() <= limit as usize)
        .ok_or(SyncError::InvalidReceipt)?;
    let cursor = serde_json::from_value(
        value
            .get("next_cursor")
            .cloned()
            .ok_or(SyncError::InvalidReceipt)?,
    )
    .map_err(|_| SyncError::InvalidReceipt)?;
    Ok((items, cursor))
}
