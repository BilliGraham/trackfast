use crate::{Entry, Error, Queue, storage, transport::RemoteChange};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct Conflict {
    pub operation_id: Uuid,
    pub local: Entry,
    pub action: String,
    pub remote: Option<RemoteChange>,
}

impl Queue {
    pub fn conflicts(&self) -> Result<Vec<Conflict>, Error> {
        let mut statement=self.connection.prepare("SELECT p.operation_id,e.body,p.action,r.body FROM pending p JOIN entries e USING(entry_id) LEFT JOIN remote_conflicts r USING(entry_id) WHERE p.conflict=1 ORDER BY p.sequence").map_err(storage)?;
        let rows = statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            })
            .map_err(storage)?;
        rows.map(|row| {
            let (key, body, action, remote) = row.map_err(storage)?;
            Ok(Conflict {
                operation_id: key.parse().map_err(storage)?,
                local: serde_json::from_str(&body).map_err(storage)?,
                action,
                remote: remote
                    .map(|s| serde_json::from_str(&s).map_err(storage))
                    .transpose()?,
            })
        })
        .collect()
    }
    pub(crate) fn has_conflicts(&self) -> Result<bool, Error> {
        self.connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pending WHERE conflict=1)",
                [],
                |r| r.get(0),
            )
            .map_err(storage)
    }
    /// Accepts only the stored remote snapshot that the user reviewed. Local pending work is discarded.
    pub fn resolve_with_remote(
        &mut self,
        operation_id: Uuid,
        reviewed_version: i64,
    ) -> Result<(), Error> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let (id, _, _, remote) = reviewed(&tx, operation_id, reviewed_version)?;
        accept(&tx, &id, operation_id, &remote)?;
        tx.commit().map_err(storage)
    }
    /// Preserves the local content under a fresh entry ID and accepts the reviewed remote original.
    /// Use this after a creation collision or remote deletion. A pending deletion cannot be copied.
    pub fn resolve_as_new(
        &mut self,
        operation_id: Uuid,
        reviewed_version: i64,
    ) -> Result<Entry, Error> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let (id, action, body, remote) = reviewed(&tx, operation_id, reviewed_version)?;
        if action == "delete" {
            return Err(Error::UnknownOperation);
        }
        let mut entry: Entry = serde_json::from_str(&body).map_err(storage)?;
        crate::validate(&entry)?;
        entry.entry_id = Uuid::new_v4();
        accept(&tx, &id, operation_id, &remote)?;
        tx.execute(
            "INSERT INTO entries(entry_id,body) VALUES(?1,?2)",
            params![
                entry.entry_id.to_string(),
                serde_json::to_string(&entry).map_err(storage)?
            ],
        )
        .map_err(storage)?;
        tx.execute(
            "INSERT INTO pending(operation_id,entry_id) VALUES(?1,?2)",
            params![Uuid::new_v4().to_string(), entry.entry_id.to_string()],
        )
        .map_err(storage)?;
        tx.commit().map_err(storage)?;
        Ok(entry)
    }
    /// Requeues local work against the remote version reviewed by the user. Uses a new operation key.
    /// A known remote deletion requires explicit acceptance or preservation as a new entry.
    pub fn resolve_with_local(
        &mut self,
        operation_id: Uuid,
        remote_version: i64,
    ) -> Result<Uuid, Error> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let row:Option<(String,String,Option<i64>)>=tx.query_row("SELECT entry_id,action,expected_version FROM pending WHERE operation_id=?1 AND conflict=1",[operation_id.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(storage)?;
        let (id, action, previous) = row.ok_or(Error::UnknownOperation)?;
        if remote_version <= previous.unwrap_or(0) || remote_version == i64::MAX {
            return Err(Error::ConflictChanged);
        }
        let remote: Option<String> = tx
            .query_row(
                "SELECT body FROM remote_conflicts WHERE entry_id=?1",
                [&id],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        if let Some(raw) = remote {
            let remote: RemoteChange = serde_json::from_str(&raw).map_err(storage)?;
            if remote.version != remote_version || remote.entry.is_none() {
                return Err(Error::ConflictChanged);
            }
            let body: String = tx
                .query_row("SELECT body FROM entries WHERE entry_id=?1", [&id], |r| {
                    r.get(0)
                })
                .map_err(storage)?;
            let local: Entry = serde_json::from_str(&body).map_err(storage)?;
            if local.recorded_settings != remote.entry.unwrap().recorded_settings {
                return Err(Error::ConflictChanged);
            }
        } else if action == "create" {
            return Err(Error::ConflictChanged);
        }
        let key = Uuid::new_v4();
        tx.execute("UPDATE pending SET operation_id=?2,expected_version=?3,conflict=0,action=CASE WHEN action='create' THEN 'replace' ELSE action END WHERE operation_id=?1",params![operation_id.to_string(),key.to_string(),remote_version]).map_err(storage)?;
        tx.execute(
            "UPDATE entries SET server_version=?2 WHERE entry_id=?1",
            params![id, remote_version],
        )
        .map_err(storage)?;
        tx.execute("DELETE FROM remote_conflicts WHERE entry_id=?1", [&id])
            .map_err(storage)?;
        tx.commit().map_err(storage)?;
        Ok(key)
    }
}
pub(crate) fn reviewed(
    tx: &Transaction<'_>,
    operation: Uuid,
    version: i64,
) -> Result<(String, String, String, RemoteChange), Error> {
    let row:Option<(String,String,String,String)>=tx.query_row("SELECT p.entry_id,p.action,e.body,r.body FROM pending p JOIN entries e USING(entry_id) JOIN remote_conflicts r USING(entry_id) WHERE p.operation_id=?1 AND p.conflict=1",[operation.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(storage)?;
    let (id, action, body, remote) = row.ok_or(Error::UnknownOperation)?;
    let remote: RemoteChange = serde_json::from_str(&remote).map_err(storage)?;
    if remote.version != version || version <= 0 {
        return Err(Error::ConflictChanged);
    }
    Ok((id, action, body, remote))
}
pub(crate) fn accept(
    tx: &Transaction<'_>,
    id: &str,
    operation: Uuid,
    remote: &RemoteChange,
) -> Result<(), Error> {
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
    tx.execute(
        "DELETE FROM pending WHERE operation_id=?1",
        [operation.to_string()],
    )
    .map_err(storage)?;
    tx.execute("DELETE FROM remote_conflicts WHERE entry_id=?1", [id])
        .map_err(storage)?;
    Ok(())
}
