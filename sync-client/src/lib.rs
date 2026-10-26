use chrono::{Datelike, NaiveDate};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
use uuid::Uuid;

const APPLICATION_ID: i32 = 0x54465351;
const MIGRATIONS: &[&str] = &[include_str!("../migrations/001_queue.sql")];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
    pub server: String,
    pub tenant_id: Uuid,
    pub person_id: Uuid,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurationEntry {
    pub entry_id: Uuid,
    pub entry_date: NaiveDate,
    pub description: String,
    pub original_seconds: u64,
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
}

fn storage<E>(_: E) -> Error {
    Error::Storage
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Storage => "The local queue operation failed.",
            Self::ScopeMismatch => "This queue belongs to another account or server.",
            Self::InvalidEntry => "Check the duration entry fields.",
            Self::DuplicateEntry => "This entry already exists in the local queue.",
            Self::UnknownOperation => "The pending operation is unavailable.",
            Self::IncompatibleDatabase => "This file is not a compatible queue database.",
        })
    }
}

impl std::error::Error for Error {}

pub struct Queue {
    connection: Connection,
}

impl Queue {
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
        Ok(Self { connection })
    }

    /// Save one immutable duration entry and its pending creation together.
    pub fn enqueue(&mut self, entry: &DurationEntry) -> Result<Uuid, Error> {
        if entry.original_seconds == 0
            || entry.original_seconds > 9_007_199_254_740_991
            || entry.description.chars().count() > 2000
            || !(1..=9999).contains(&entry.entry_date.year())
        {
            return Err(Error::InvalidEntry);
        }
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
        let changed = self
            .connection
            .execute(
                "DELETE FROM pending WHERE operation_id = ?1",
                [operation_id.to_string()],
            )
            .map_err(storage)?;
        if changed == 0 {
            return Err(Error::UnknownOperation);
        }
        Ok(())
    }

    pub fn entry(&self, entry_id: Uuid) -> Result<Option<DurationEntry>, Error> {
        let body: Option<String> = self
            .connection
            .query_row(
                "SELECT body FROM entries WHERE entry_id = ?1",
                [entry_id.to_string()],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        body.map(|body| serde_json::from_str(&body).map_err(storage))
            .transpose()
    }
}

#[cfg(test)]
mod tests;
