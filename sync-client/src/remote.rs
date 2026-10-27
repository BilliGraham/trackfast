use crate::{Error, Queue, storage, transport::RemoteChange};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use uuid::Uuid;

impl Queue {
    pub fn checkpoint(&self) -> Result<Option<Uuid>, Error> {
        let cursor: Option<String> = self
            .connection
            .query_row("SELECT cursor FROM remote_checkpoint WHERE id=1", [], |r| {
                r.get(0)
            })
            .map_err(storage)?;
        cursor.map(|s| s.parse().map_err(storage)).transpose()
    }
    pub fn remote_conflict(&self, id: Uuid) -> Result<Option<RemoteChange>, Error> {
        let body: Option<String> = self
            .connection
            .query_row(
                "SELECT body FROM remote_conflicts WHERE entry_id=?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        body.map(|s| serde_json::from_str(&s).map_err(storage))
            .transpose()
    }
    pub(crate) fn apply_remote_page(
        &mut self,
        previous: Option<Uuid>,
        next: Uuid,
        changes: &[RemoteChange],
    ) -> Result<(), Error> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let cursor: Option<String> = tx
            .query_row("SELECT cursor FROM remote_checkpoint WHERE id=1", [], |r| {
                r.get(0)
            })
            .map_err(storage)?;
        if cursor != previous.map(|v| v.to_string()) {
            return Err(Error::PendingOperation);
        }
        for change in changes {
            if change.version <= 0 {
                return Err(Error::InvalidEntry);
            }
            if let Some(entry) = &change.entry {
                crate::validate(entry)?;
                if entry.entry_id != change.entry_id {
                    return Err(Error::InvalidEntry);
                }
            }
            let id = change.entry_id.to_string();
            let version: Option<i64> = tx
                .query_row(
                    "SELECT server_version FROM entries WHERE entry_id=?1",
                    [&id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(storage)?;
            if version.is_some_and(|v| v >= change.version) {
                continue;
            }
            let stored: Option<String> = tx
                .query_row(
                    "SELECT body FROM remote_conflicts WHERE entry_id=?1",
                    [&id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(storage)?;
            if let Some(stored) = stored {
                let stored: RemoteChange = serde_json::from_str(&stored).map_err(storage)?;
                if stored.version >= change.version {
                    continue;
                }
            }
            let pending: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM pending WHERE entry_id=?1)",
                    [&id],
                    |r| r.get(0),
                )
                .map_err(storage)?;
            if pending {
                tx.execute("INSERT INTO remote_conflicts VALUES(?1,?2) ON CONFLICT(entry_id) DO UPDATE SET body=excluded.body",params![id,serde_json::to_string(change).map_err(storage)?]).map_err(storage)?;
                tx.execute("UPDATE pending SET conflict=1 WHERE entry_id=?1", [&id])
                    .map_err(storage)?;
            } else {
                tx.execute("INSERT INTO entries(entry_id,body,server_version,deleted) VALUES(?1,?2,?3,?4) ON CONFLICT(entry_id) DO UPDATE SET body=excluded.body,server_version=excluded.server_version,deleted=excluded.deleted",params![id,serde_json::to_string(&change.entry).map_err(storage)?,change.version,change.entry.is_none()]).map_err(storage)?;
                tx.execute("DELETE FROM remote_conflicts WHERE entry_id=?1", [&id])
                    .map_err(storage)?;
            }
        }
        tx.execute(
            "UPDATE remote_checkpoint SET cursor=?1 WHERE id=1",
            [next.to_string()],
        )
        .map_err(storage)?;
        tx.commit().map_err(storage)
    }
}
