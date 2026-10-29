use crate::{
    Error, Queue, storage,
    transfer::Transfer,
    transport::{ChangePage, RemoteChange},
};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Incoming {
    pub id: Uuid,
    pub change: RemoteChange,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RemoteOutcome {
    Applied,
    Conflict(Transfer),
}

#[derive(Serialize, Deserialize)]
struct Download {
    cursor: Uuid,
    more: bool,
    items: VecDeque<Incoming>,
}

impl Queue {
    pub fn native_download_cursor(&self) -> Result<Option<Uuid>, Error> {
        let raw: Option<String> = self
            .connection
            .query_row(
                "SELECT cursor FROM native_download_cursor WHERE id=1",
                [],
                |r| r.get(0),
            )
            .map_err(storage)?;
        raw.map(|s| s.parse().map_err(storage)).transpose()
    }
    pub fn stage_native_page(
        &mut self,
        previous: Option<Uuid>,
        page: ChangePage,
    ) -> Result<(), Error> {
        if page.cursor.is_nil()
            || page.changes.len() > 100
            || (page.more && page.changes.is_empty())
            || (!page.changes.is_empty() && Some(page.cursor) == previous)
        {
            return Err(Error::InvalidEntry);
        }
        for change in &page.changes {
            if change.entry_id.is_nil() || change.version <= 0 {
                return Err(Error::InvalidEntry);
            }
            if let Some(entry) = &change.entry {
                crate::validate(entry)?;
                if entry.entry_id != change.entry_id {
                    return Err(Error::InvalidEntry);
                }
            }
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let cursor: Option<String> = tx
            .query_row(
                "SELECT cursor FROM native_download_cursor WHERE id=1",
                [],
                |r| r.get(0),
            )
            .map_err(storage)?;
        if cursor != previous.map(|v| v.to_string()) || load(&tx)?.is_some() {
            return Err(Error::PendingOperation);
        }
        let download = Download {
            cursor: page.cursor,
            more: page.more,
            items: page
                .changes
                .into_iter()
                .map(|change| Incoming {
                    id: Uuid::new_v4(),
                    change,
                })
                .collect(),
        };
        save(&tx, &download)?;
        tx.commit().map_err(storage)
    }
    /// Returns one unapplied change. Old API versions do not replace newer confirmed baselines.
    pub fn next_native_remote(&mut self) -> Result<Option<Incoming>, Error> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let Some(mut download) = load(&tx)? else {
            return Ok(None);
        };
        while let Some(incoming) = download.items.front() {
            let version: Option<i64> = tx
                .query_row(
                    "SELECT server_version FROM entries WHERE entry_id=?1",
                    [incoming.change.entry_id.to_string()],
                    |r| r.get(0),
                )
                .optional()
                .map_err(storage)?;
            if version.is_some_and(|v| v >= incoming.change.version) {
                download.items.pop_front();
            } else {
                break;
            }
        }
        let next = download.items.front().cloned();
        save(&tx, &download)?;
        tx.commit().map_err(storage)?;
        Ok(next)
    }
    /// Updates the queue baseline only after the native transaction has returned its durable result.
    pub fn acknowledge_native_remote(
        &mut self,
        incoming: &Incoming,
        outcome: &RemoteOutcome,
    ) -> Result<bool, Error> {
        let done: bool = self
            .connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM native_remote_acks WHERE token=?1)",
                [incoming.id.to_string()],
                |r| r.get(0),
            )
            .map_err(storage)?;
        if done {
            return Ok(false);
        }
        if self.next_native_remote()?.as_ref() != Some(incoming) {
            return Err(Error::UnknownOperation);
        }
        if let RemoteOutcome::Conflict(transfer) = outcome {
            if transfer.entry_id != incoming.change.entry_id {
                return Err(Error::InvalidEntry);
            }
            if self.stage_transfer(transfer)? {
                return Err(Error::UnknownOperation);
            }
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let mut download = load(&tx)?.ok_or(Error::UnknownOperation)?;
        if download.items.front() != Some(incoming) {
            return Err(Error::UnknownOperation);
        }
        let change = &incoming.change;
        let id = change.entry_id.to_string();
        match outcome {
            RemoteOutcome::Applied => {
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
                tx.execute("INSERT INTO entries(entry_id,body,server_version,deleted) VALUES(?1,?2,?3,?4) ON CONFLICT(entry_id) DO UPDATE SET body=excluded.body,server_version=excluded.server_version,deleted=excluded.deleted",params![id,serde_json::to_string(&change.entry).map_err(storage)?,change.version,change.entry.is_none()]).map_err(storage)?;
            }
            RemoteOutcome::Conflict(transfer) => {
                tx.execute(
                    "UPDATE pending SET conflict=1 WHERE operation_id=?1",
                    [transfer.operation_id.to_string()],
                )
                .map_err(storage)?;
                tx.execute("INSERT INTO remote_conflicts VALUES(?1,?2) ON CONFLICT(entry_id) DO UPDATE SET body=excluded.body",params![id,serde_json::to_string(change).map_err(storage)?]).map_err(storage)?;
            }
        }
        tx.execute(
            "INSERT INTO native_remote_acks VALUES(?1)",
            [incoming.id.to_string()],
        )
        .map_err(storage)?;
        download.items.pop_front();
        let more = !download.items.is_empty() || download.more;
        save(&tx, &download)?;
        tx.commit().map_err(storage)?;
        Ok(more)
    }
}
fn load(tx: &Transaction<'_>) -> Result<Option<Download>, Error> {
    let body: Option<String> = tx
        .query_row("SELECT body FROM native_download WHERE id=1", [], |r| {
            r.get(0)
        })
        .optional()
        .map_err(storage)?;
    body.map(|s| serde_json::from_str(&s).map_err(storage))
        .transpose()
}
fn save(tx: &Transaction<'_>, download: &Download) -> Result<(), Error> {
    if download.items.is_empty() {
        tx.execute(
            "UPDATE native_download_cursor SET cursor=?1 WHERE id=1",
            [download.cursor.to_string()],
        )
        .map_err(storage)?;
        tx.execute("DELETE FROM native_download WHERE id=1", [])
            .map_err(storage)?;
    } else {
        tx.execute("INSERT INTO native_download VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET body=excluded.body",[serde_json::to_string(download).map_err(storage)?]).map_err(storage)?;
    }
    Ok(())
}
