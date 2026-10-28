use crate::{Entry, Error, Queue, storage, validate};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Change {
    Create(Entry),
    Replace(Entry),
    Delete,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transfer {
    pub operation_id: Uuid,
    pub entry_id: Uuid,
    pub change: Change,
}

impl Queue {
    /// Stages an immutable native operation. Returns true after a verified receipt or a saved reviewed resolution.
    /// Call `native_resolution` before acknowledging the result in the native store.
    /// Repeated transfers must have the same content and operation ID.
    pub fn stage_transfer(&mut self, transfer: &Transfer) -> Result<bool, Error> {
        if transfer.operation_id.is_nil() || transfer.entry_id.is_nil() {
            return Err(Error::InvalidEntry);
        }
        let fingerprint = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(transfer).map_err(storage)?)
        );
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let key = transfer.operation_id.to_string();
        let id = transfer.entry_id.to_string();
        let prior: Option<(String, bool)> = tx
            .query_row(
                "SELECT fingerprint,confirmed FROM native_transfers WHERE operation_id=?1",
                [&key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(storage)?;
        if let Some((prior, confirmed)) = prior {
            if prior != fingerprint {
                return Err(Error::InvalidEntry);
            }
            return Ok(confirmed);
        }
        if tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pending WHERE entry_id=?1)",
                [&id],
                |r| r.get::<_, bool>(0),
            )
            .map_err(storage)?
        {
            return Err(Error::PendingOperation);
        }
        let entry = match &transfer.change {
            Change::Create(e) | Change::Replace(e) => Some(e),
            Change::Delete => None,
        };
        if let Some(entry) = entry {
            validate(entry)?;
            if entry.entry_id != transfer.entry_id {
                return Err(Error::InvalidEntry);
            }
        }
        let (action, version) = match &transfer.change {
            Change::Create(entry) => {
                let inserted = tx.execute("INSERT INTO entries(entry_id,body) VALUES (?1,?2) ON CONFLICT(entry_id) DO NOTHING", params![id,serde_json::to_string(entry).map_err(storage)?]).map_err(storage)?;
                if inserted == 0 {
                    return Err(Error::DuplicateEntry);
                }
                ("create", None)
            }
            Change::Replace(_) | Change::Delete => {
                let saved: Option<(i64, String)> = tx
                    .query_row(
                        "SELECT server_version,body FROM entries WHERE entry_id=?1 AND NOT deleted",
                        [&id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()
                    .map_err(storage)?;
                let (version, previous) = saved
                    .filter(|(v, _)| *v > 0 && *v < i64::MAX)
                    .ok_or(Error::UnknownEntry)?;
                if let Some(entry) = entry {
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
                (
                    if entry.is_some() { "replace" } else { "delete" },
                    Some(version),
                )
            }
        };
        tx.execute("INSERT INTO pending(operation_id,entry_id,action,expected_version) VALUES (?1,?2,?3,?4)", params![key,id,action,version]).map_err(storage)?;
        tx.execute(
            "INSERT INTO native_transfers(operation_id,fingerprint) VALUES (?1,?2)",
            params![key, fingerprint],
        )
        .map_err(storage)?;
        tx.commit().map_err(storage)?;
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Entry, Queue, RecordedSettings, Scope};
    use uuid::Uuid;

    #[test]
    fn replay_preserves_the_key_and_confirms_only_after_a_receipt() {
        let scope = Scope {
            server: "http://127.0.0.1:8788".into(),
            tenant_id: Uuid::new_v4(),
            person_id: Uuid::new_v4(),
        };
        let mut queue =
            Queue::prepare(rusqlite::Connection::open_in_memory().unwrap(), &scope).unwrap();
        let transfer = Transfer {
            operation_id: Uuid::new_v4(),
            entry_id: Uuid::new_v4(),
            change: Change::Create(Entry {
                entry_id: Uuid::nil(),
                entry_date: "2026-10-28".parse().unwrap(),
                description: "Work".into(),
                original_seconds: 781,
                recorded_settings: Some(RecordedSettings {
                    rounding_interval_minutes: Some(5),
                }),
                project_id: None,
                timing: None,
            }),
        };
        let mut transfer = transfer;
        if let Change::Create(entry) = &mut transfer.change {
            entry.entry_id = transfer.entry_id;
        }
        assert!(!queue.stage_transfer(&transfer).unwrap());
        assert!(!queue.stage_transfer(&transfer).unwrap());
        assert_eq!(
            queue.pending().unwrap()[0].operation_id,
            transfer.operation_id
        );
        assert_eq!(queue.pending().unwrap().len(), 1);
        let mut bad = transfer.clone();
        if let Change::Create(entry) = &mut bad.change {
            entry.description = "Changed replay".into();
        }
        assert!(queue.stage_transfer(&bad).is_err());
        queue.confirm(transfer.operation_id).unwrap();
        assert!(queue.stage_transfer(&transfer).unwrap());
        let mut replace = transfer.clone();
        replace.operation_id = Uuid::new_v4();
        if let Change::Create(mut entry) = replace.change {
            entry.description = "Edited".into();
            replace.change = Change::Replace(entry);
        }
        assert!(!queue.stage_transfer(&replace).unwrap());
        queue.confirm(replace.operation_id).unwrap();
        assert!(queue.stage_transfer(&replace).unwrap());
        let deletion = Transfer {
            operation_id: Uuid::new_v4(),
            entry_id: transfer.entry_id,
            change: Change::Delete,
        };
        assert!(!queue.stage_transfer(&deletion).unwrap());
        queue.confirm(deletion.operation_id).unwrap();
        assert!(queue.stage_transfer(&deletion).unwrap());
        assert!(queue.entry(transfer.entry_id).unwrap().is_none());
    }

    #[test]
    fn transfer_and_receipt_storage_failures_roll_back_the_whole_change() {
        let scope = Scope {
            server: "http://127.0.0.1:8788".into(),
            tenant_id: Uuid::new_v4(),
            person_id: Uuid::new_v4(),
        };
        let mut q =
            Queue::prepare(rusqlite::Connection::open_in_memory().unwrap(), &scope).unwrap();
        let id = Uuid::new_v4();
        let transfer = Transfer {
            operation_id: Uuid::new_v4(),
            entry_id: id,
            change: Change::Create(Entry {
                entry_id: id,
                entry_date: "2026-10-28".parse().unwrap(),
                description: "Work".into(),
                original_seconds: 60,
                recorded_settings: None,
                project_id: None,
                timing: None,
            }),
        };
        q.connection.execute_batch("CREATE TRIGGER fail_transfer BEFORE INSERT ON native_transfers BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
        assert!(q.stage_transfer(&transfer).is_err());
        assert!(q.pending().unwrap().is_empty());
        assert!(q.entry(id).unwrap().is_none());
        q.connection
            .execute_batch("DROP TRIGGER fail_transfer;")
            .unwrap();
        q.stage_transfer(&transfer).unwrap();
        q.connection.execute_batch("CREATE TRIGGER fail_receipt BEFORE UPDATE ON native_transfers BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
        assert!(q.confirm(transfer.operation_id).is_err());
        assert_eq!(q.pending().unwrap().len(), 1);
        assert!(!q.stage_transfer(&transfer).unwrap());
    }
}
