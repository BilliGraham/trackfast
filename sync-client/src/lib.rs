use chrono::{Datelike, NaiveDate};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
use uuid::Uuid;

mod entry;
pub mod native_download;
#[cfg(test)]
mod native_download_tests;
pub mod native_resolution;
#[cfg(test)]
mod native_resolution_tests;
mod remote;
mod resolution;
pub mod scheduler;
pub mod transfer;
pub use resolution::Conflict;
pub mod session;
pub mod transport;

const APPLICATION_ID: i32 = 0x54465351;
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/001_queue.sql"),
    include_str!("../migrations/002_changes.sql"),
    include_str!("../migrations/003_remote.sql"),
    include_str!("../migrations/004_schedule.sql"),
    include_str!("../migrations/005_transfers.sql"),
    include_str!("../migrations/006_native_resolutions.sql"),
    include_str!("../migrations/007_native_download.sql"),
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
    pub server: String,
    pub tenant_id: Uuid,
    pub person_id: Uuid,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorded_settings: Option<RecordedSettings>,
    #[serde(default)]
    pub project_id: Option<Uuid>,
    #[serde(default)]
    pub timing: Option<Timing>,
    pub entry_id: Uuid,
    pub entry_date: NaiveDate,
    pub description: String,
    pub original_seconds: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordedSettings {
    pub rounding_interval_minutes: Option<u8>,
}

/// Compatibility name for callers that already use duration entries.
pub type DurationEntry = Entry;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timing {
    pub started_at: String,
    pub stopped_at: String,
    pub time_zone: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingCreate {
    pub operation_id: Uuid,
    pub entry: DurationEntry,
}

#[derive(Debug)]
pub enum Error {
    Storage,
    ScopeMismatch,
    InvalidEntry,
    DuplicateEntry,
    UnknownOperation,
    IncompatibleDatabase,
    PendingOperation,
    UnknownEntry,
    ConflictChanged,
}

fn storage<E>(_: E) -> Error {
    Error::Storage
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Storage => "The local queue operation failed.",
            Self::ScopeMismatch => "This queue belongs to another account or server.",
            Self::InvalidEntry => "Check the entry fields.",
            Self::DuplicateEntry => "This entry already exists in the local queue.",
            Self::UnknownOperation => "The pending operation is unavailable.",
            Self::IncompatibleDatabase => "This file is not a compatible queue database.",
            Self::PendingOperation => {
                "Confirm or resolve the pending operation before another change."
            }
            Self::UnknownEntry => "The confirmed local entry is unavailable.",
            Self::ConflictChanged => "The conflict changed. Review its current remote version.",
        })
    }
}

impl std::error::Error for Error {}

pub struct Queue {
    connection: Connection,
    scope: Scope,
}

impl Queue {
    fn mark_conflict(&mut self, operation_id: Uuid) -> Result<(), Error> {
        self.connection
            .execute(
                "UPDATE pending SET conflict=1 WHERE operation_id=?1",
                [operation_id.to_string()],
            )
            .map_err(storage)?;
        Ok(())
    }

    pub fn replace(&mut self, entry: &DurationEntry) -> Result<Uuid, Error> {
        validate(entry)?;
        self.change(entry.entry_id, Some(entry))
    }

    pub fn delete(&mut self, entry_id: Uuid) -> Result<Uuid, Error> {
        self.change(entry_id, None)
    }

    fn change(
        &mut self,
        entry_id: Uuid,
        replacement: Option<&DurationEntry>,
    ) -> Result<Uuid, Error> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let id = entry_id.to_string();
        let pending: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pending WHERE entry_id=?1)",
                [&id],
                |r| r.get(0),
            )
            .map_err(storage)?;
        if pending {
            return Err(Error::PendingOperation);
        }
        let version: Option<i64> = tx
            .query_row(
                "SELECT server_version FROM entries WHERE entry_id=?1 AND NOT deleted",
                [&id],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        let version = version
            .filter(|v| *v > 0 && *v < i64::MAX)
            .ok_or(Error::UnknownEntry)?;
        if let Some(entry) = replacement {
            let previous: String = tx
                .query_row("SELECT body FROM entries WHERE entry_id=?1", [&id], |r| {
                    r.get(0)
                })
                .map_err(storage)?;
            let previous: Entry = serde_json::from_str(&previous).map_err(storage)?;
            if previous.recorded_settings != entry.recorded_settings {
                return Err(Error::InvalidEntry);
            }
            tx.execute(
                "UPDATE entries SET body=?2 WHERE entry_id=?1",
                params![id, serde_json::to_string(entry).map_err(storage)?],
            )
            .map_err(storage)?;
        }
        let key = Uuid::new_v4();
        tx.execute("INSERT INTO pending(operation_id,entry_id,action,expected_version) VALUES (?1,?2,?3,?4)", params![key.to_string(),id,if replacement.is_some() {"replace"} else {"delete"},version]).map_err(storage)?;
        tx.commit().map_err(storage)?;
        Ok(key)
    }

    fn operation_details(&self, key: Uuid) -> Result<(String, Option<i64>), Error> {
        self.connection
            .query_row(
                "SELECT action,expected_version FROM pending WHERE operation_id=?1",
                [key.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(storage)
    }

    /// Open a separate queue database from its future database worker thread.
    /// Never use the native app database path here.
    pub fn open(path: &Path, scope: &Scope) -> Result<Self, Error> {
        let connection = Connection::open(path).map_err(|_| Error::Storage)?;
        Self::prepare(connection, scope)
    }

    fn prepare(mut connection: Connection, scope: &Scope) -> Result<Self, Error> {
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(storage)?;
        connection
            .execute_batch("PRAGMA foreign_keys = ON; PRAGMA synchronous = FULL;")
            .map_err(storage)?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let application: i32 = tx
            .query_row("PRAGMA application_id", [], |r| r.get(0))
            .map_err(storage)?;
        let version: i32 = tx
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(storage)?;
        let tables: i64 = tx
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
                [],
                |r| r.get(0),
            )
            .map_err(storage)?;
        if (application != APPLICATION_ID && (application != 0 || tables != 0 || version != 0))
            || version < 0
            || version as usize > MIGRATIONS.len()
        {
            return Err(Error::IncompatibleDatabase);
        }
        for (index, migration) in MIGRATIONS.iter().enumerate().skip(version as usize) {
            tx.execute_batch(migration).map_err(storage)?;
            tx.pragma_update(None, "user_version", (index + 1) as i32)
                .map_err(storage)?;
        }
        tx.pragma_update(None, "application_id", APPLICATION_ID)
            .map_err(storage)?;
        let encoded = serde_json::to_string(scope).map_err(storage)?;
        tx.execute(
            "INSERT OR IGNORE INTO scope(id, value) VALUES (1, ?1)",
            [&encoded],
        )
        .map_err(storage)?;
        let stored: String = tx
            .query_row("SELECT value FROM scope WHERE id = 1", [], |r| r.get(0))
            .map_err(storage)?;
        if stored != encoded {
            return Err(Error::ScopeMismatch);
        }
        tx.commit().map_err(storage)?;
        Ok(Self {
            connection,
            scope: scope.clone(),
        })
    }

    /// Save one immutable entry and its pending creation together.
    pub fn enqueue(&mut self, entry: &DurationEntry) -> Result<Uuid, Error> {
        validate(entry)?;
        let body = serde_json::to_string(entry).map_err(storage)?;
        let operation_id = Uuid::new_v4();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let inserted = tx.execute("INSERT INTO entries(entry_id, body) VALUES (?1, ?2) ON CONFLICT(entry_id) DO NOTHING", params![entry.entry_id.to_string(), body]).map_err(storage)?;
        if inserted == 0 {
            return Err(Error::DuplicateEntry);
        }
        tx.execute(
            "INSERT INTO pending(operation_id, entry_id) VALUES (?1, ?2)",
            params![operation_id.to_string(), entry.entry_id.to_string()],
        )
        .map_err(storage)?;
        tx.commit().map_err(storage)?;
        Ok(operation_id)
    }

    /// Read pending operations without changing their keys or contents.
    pub fn pending(&self) -> Result<Vec<PendingCreate>, Error> {
        let mut statement = self.connection.prepare("SELECT operation_id, body FROM pending JOIN entries USING(entry_id) ORDER BY pending.sequence").map_err(storage)?;
        let rows = statement
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(storage)?;
        rows.map(|row| {
            let (key, body) = row.map_err(storage)?;
            Ok(PendingCreate {
                operation_id: Uuid::parse_str(&key).map_err(storage)?,
                entry: serde_json::from_str(&body).map_err(storage)?,
            })
        })
        .collect()
    }

    /// Remove an operation only after the transport confirms its server receipt.
    /// The caller must first verify the authenticated identity and receipt contents.
    pub fn confirm(&mut self, operation_id: Uuid) -> Result<(), Error> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let key = operation_id.to_string();
        let operation: Option<(String, String, Option<i64>)> = tx
            .query_row(
                "SELECT entry_id,action,expected_version FROM pending WHERE operation_id=?1",
                [&key],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(storage)?;
        let (id, action, expected) = operation.ok_or(Error::UnknownOperation)?;
        let version = expected.unwrap_or(0).checked_add(1).ok_or(Error::Storage)?;
        tx.execute(
            "UPDATE entries SET server_version=?2,deleted=?3 WHERE entry_id=?1",
            params![id, version, action == "delete"],
        )
        .map_err(storage)?;
        let remote: Option<String> = tx
            .query_row(
                "SELECT body FROM remote_conflicts WHERE entry_id=?1",
                [&id],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        if let Some(remote) = remote {
            let remote: transport::RemoteChange = serde_json::from_str(&remote).map_err(storage)?;
            if remote.version > version {
                tx.execute(
                    "UPDATE entries SET body=?2,server_version=?3,deleted=?4 WHERE entry_id=?1",
                    params![
                        id,
                        serde_json::to_string(&remote.entry).map_err(storage)?,
                        remote.version,
                        remote.entry.is_none()
                    ],
                )
                .map_err(storage)?;
            }
        }
        tx.execute("DELETE FROM remote_conflicts WHERE entry_id=?1", [&id])
            .map_err(storage)?;
        tx.execute("DELETE FROM pending WHERE operation_id=?1", [&key])
            .map_err(storage)?;
        tx.execute(
            "UPDATE native_transfers SET confirmed=1 WHERE operation_id=?1",
            [&key],
        )
        .map_err(storage)?;
        tx.execute("UPDATE native_transfers SET confirmed=1 WHERE operation_id IN (SELECT operation_id FROM native_resolutions WHERE retry_operation_id=?1)", [&key]).map_err(storage)?;
        tx.commit().map_err(storage)?;
        Ok(())
    }

    pub fn entry(&self, entry_id: Uuid) -> Result<Option<DurationEntry>, Error> {
        let body: Option<String> = self
            .connection
            .query_row(
                "SELECT body FROM entries WHERE entry_id = ?1 AND NOT deleted",
                [entry_id.to_string()],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        body.map(|body| serde_json::from_str(&body).map_err(storage))
            .transpose()
    }
}

/// Checks entry content before it becomes an immutable queue operation.
pub fn validate(entry: &DurationEntry) -> Result<(), Error> {
    if entry
        .recorded_settings
        .as_ref()
        .is_some_and(|s| !matches!(s.rounding_interval_minutes, None | Some(5)))
    {
        return Err(Error::InvalidEntry);
    }
    if let Some(timing) = &entry.timing {
        timing.validate(entry)?;
    }
    if entry.original_seconds == 0
        || entry.original_seconds > 9_007_199_254_740_991
        || entry.description.chars().count() > 2000
        || !(1..=9999).contains(&entry.entry_date.year())
    {
        return Err(Error::InvalidEntry);
    }
    Ok(())
}

#[cfg(test)]
mod reliability_tests;
#[cfg(test)]
mod tests;
