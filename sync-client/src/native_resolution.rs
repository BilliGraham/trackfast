use crate::{Conflict, Error, Queue, resolution, storage, transport::RemoteChange};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Choice {
    Local,
    Remote,
    Copy,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Resolution {
    pub choice: Choice,
    pub remote: RemoteChange,
}

impl Queue {
    pub fn native_resolution(&self, operation: Uuid) -> Result<Option<Resolution>, Error> {
        let body: Option<String> = self
            .connection
            .query_row(
                "SELECT body FROM native_resolutions WHERE operation_id=?1",
                [operation.to_string()],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        body.map(|s| serde_json::from_str(&s).map_err(storage))
            .transpose()
    }

    /// Returns native conflicts under their original, immutable transfer keys.
    pub fn native_conflicts(&self) -> Result<Vec<Conflict>, Error> {
        let mut result = Vec::new();
        for mut conflict in self.conflicts()? {
            let original: Option<String> = self.connection.query_row("SELECT operation_id FROM native_transfers WHERE operation_id=?1 UNION SELECT operation_id FROM native_resolutions WHERE retry_operation_id=?1", [conflict.operation_id.to_string()], |r|r.get(0)).optional().map_err(storage)?;
            if let Some(original) = original {
                conflict.operation_id = original.parse().map_err(storage)?;
                result.push(conflict);
            }
        }
        Ok(result)
    }

    /// Stores only snapshots for existing conflicts. It does not change entry baselines or feed checkpoints.
    pub fn record_native_conflicts(&mut self, changes: &[RemoteChange]) -> Result<(), Error> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        for remote in changes {
            if remote.version <= 0 {
                return Err(Error::InvalidEntry);
            }
            if let Some(entry) = &remote.entry {
                crate::validate(entry)?;
                if entry.entry_id != remote.entry_id {
                    return Err(Error::InvalidEntry);
                }
            }
            let id = remote.entry_id.to_string();
            let pending: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM pending WHERE entry_id=?1 AND conflict=1)",
                    [&id],
                    |r| r.get(0),
                )
                .map_err(storage)?;
            if !pending {
                continue;
            }
            let old: Option<String> = tx
                .query_row(
                    "SELECT body FROM remote_conflicts WHERE entry_id=?1",
                    [&id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(storage)?;
            if let Some(old) = old {
                let old: RemoteChange = serde_json::from_str(&old).map_err(storage)?;
                if old.version >= remote.version {
                    continue;
                }
            }
            tx.execute("INSERT INTO remote_conflicts VALUES(?1,?2) ON CONFLICT(entry_id) DO UPDATE SET body=excluded.body", params![id, serde_json::to_string(remote).map_err(storage)?]).map_err(storage)?;
        }
        tx.commit().map_err(storage)
    }

    /// Saves a reviewed choice and its retry or acceptance in one transaction.
    pub fn resolve_native(
        &mut self,
        original: Uuid,
        version: i64,
        choice: Choice,
    ) -> Result<(), Error> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let key = original.to_string();
        let available: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM native_transfers WHERE operation_id=?1 AND confirmed=0)", [&key], |r|r.get(0)).map_err(storage)?;
        if !available {
            return Err(Error::UnknownOperation);
        }
        let retry: Option<String> = tx
            .query_row(
                "SELECT retry_operation_id FROM native_resolutions WHERE operation_id=?1",
                [&key],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?
            .flatten();
        let pending = retry.as_deref().unwrap_or(&key).parse().map_err(storage)?;
        let (id, action, body, remote) = resolution::reviewed(&tx, pending, version)?;
        let expected: Option<i64> = tx
            .query_row(
                "SELECT expected_version FROM pending WHERE operation_id=?1",
                [pending.to_string()],
                |r| r.get(0),
            )
            .map_err(storage)?;
        if version <= expected.unwrap_or(0) {
            return Err(Error::ConflictChanged);
        }
        let retry = if choice == Choice::Local {
            let local: crate::Entry = serde_json::from_str(&body).map_err(storage)?;
            let live = remote.entry.as_ref().ok_or(Error::ConflictChanged)?;
            if local.recorded_settings != live.recorded_settings || version == i64::MAX {
                return Err(Error::ConflictChanged);
            }
            let next = Uuid::new_v4().to_string();
            tx.execute("UPDATE pending SET operation_id=?2,expected_version=?3,conflict=0,action=CASE WHEN action='create' THEN 'replace' ELSE action END WHERE operation_id=?1", params![pending.to_string(),next,version]).map_err(storage)?;
            tx.execute(
                "UPDATE entries SET server_version=?2 WHERE entry_id=?1",
                params![id, version],
            )
            .map_err(storage)?;
            tx.execute("DELETE FROM remote_conflicts WHERE entry_id=?1", [&id])
                .map_err(storage)?;
            Some(next)
        } else {
            if choice == Choice::Copy && action == "delete" {
                return Err(Error::UnknownOperation);
            }
            resolution::accept(&tx, &id, pending, &remote)?;
            tx.execute(
                "UPDATE native_transfers SET confirmed=1 WHERE operation_id=?1",
                [&key],
            )
            .map_err(storage)?;
            None
        };
        let result = Resolution { choice, remote };
        tx.execute("INSERT INTO native_resolutions VALUES(?1,?2,?3) ON CONFLICT(operation_id) DO UPDATE SET retry_operation_id=excluded.retry_operation_id,body=excluded.body", params![key,retry,serde_json::to_string(&result).map_err(storage)?]).map_err(storage)?;
        tx.commit().map_err(storage)
    }
}
