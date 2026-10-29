use super::*;
use sha2::{Digest, Sha256};
use trackfast_sync::{
    Scope,
    native_download::{Incoming, RemoteOutcome},
    transfer::Transfer,
};

impl Store {
    pub fn apply_remote(&mut self, scope: &Scope, incoming: &Incoming) -> Result<RemoteOutcome> {
        let invalid = || Error("The downloaded entry failed validation.".into());
        let change = &incoming.change;
        if incoming.id.is_nil() || change.entry_id.is_nil() || change.version <= 0 {
            return Err(invalid());
        }
        if let Some(entry) = &change.entry {
            trackfast_sync::validate(entry).map_err(|_| invalid())?;
            if entry.entry_id != change.entry_id {
                return Err(invalid());
            }
        }
        let fingerprint = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(incoming).map_err(|_| invalid())?)
        );
        let token = incoming.id.to_string();
        let tx = self.connection.transaction()?;
        enrollment::require_scope(&tx, scope)?;
        let receipt: Option<(String, String)> = tx
            .query_row(
                "SELECT fingerprint,outcome FROM sync_remote_receipts WHERE token=?1",
                [&token],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((saved, outcome)) = receipt {
            if saved != fingerprint {
                return Err(invalid());
            }
            return serde_json::from_str(&outcome).map_err(|_| invalid());
        }
        let key = change.entry_id.simple().to_string();
        let identity: Option<(Option<i64>,i64,i64,i64)> = tx.query_row("SELECT i.session_id,i.revision,COALESCE(d.revision,0),i.sequence FROM session_sync_identity i LEFT JOIN sync_delivered d ON d.entry_key=i.entry_key WHERE i.entry_key=?1", [&key], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        let previous: i64 = tx
            .query_row(
                "SELECT version FROM sync_remote_versions WHERE entry_key=?1",
                [&key],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0);
        if let Some((_, _, _, sequence)) = identity {
            let boundary: i64 =
                tx.query_row("SELECT boundary FROM sync_enrollment WHERE id=1", [], |r| {
                    r.get(0)
                })?;
            if sequence <= boundary {
                return Err(Error("The API identity matches local history. Delivery stopped without changing that history.".into()));
            }
        }
        let outcome = if previous >= change.version {
            RemoteOutcome::Applied
        } else if let Some((session_id, revision, confirmed, _)) =
            identity.filter(|(_, r, d, _)| r > d)
        {
            if let Some(id) = session_id {
                let eligible: bool = tx.query_row(
                    "SELECT stop_utc IS NOT NULL AND toggl_id IS NULL FROM sessions WHERE id=?1",
                    [id],
                    |r| r.get(0),
                )?;
                if !eligible {
                    return Err(Error(
                        "Stop the local timer before receiving this API change.".into(),
                    ));
                }
            }
            let pending: Option<(String, String)> = tx
                .query_row(
                    "SELECT entry_key,body FROM sync_outbox WHERE id=1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            let transfer = if let Some((pending_key, body)) = pending {
                if pending_key != key {
                    return Err(Error("Another local change must finish delivery before this API change can be reviewed.".into()));
                }
                serde_json::from_str::<Transfer>(&body).map_err(|_| invalid())?
            } else {
                delivery::freeze_delivery(&tx, &key, revision, session_id, confirmed)?
            };
            tx.execute("UPDATE sync_outbox SET needs_attention=1 WHERE id=1", [])?;
            RemoteOutcome::Conflict(transfer)
        } else {
            let session_id = identity.and_then(|v| v.0);
            if let Some(entry) = &change.entry {
                let preferred = session_id
                    .map(|id| {
                        tx.query_row("SELECT project_id FROM sessions WHERE id=?1", [id], |r| {
                            r.get::<_, Option<i64>>(0)
                        })
                    })
                    .transpose()?
                    .flatten();
                let session = resolution::from_remote(&tx, change, preferred)?;
                let id = resolution::write_session(&tx, &session, session_id)?;
                if identity.is_none() {
                    tx.execute(
                        "UPDATE session_sync_identity SET entry_key=?1 WHERE session_id=?2",
                        params![key, id],
                    )?;
                } else if session_id.is_none() {
                    tx.execute(
                        "DELETE FROM session_sync_identity WHERE session_id=?1",
                        [id],
                    )?;
                    tx.execute("UPDATE session_sync_identity SET session_id=?2,revision=revision+1 WHERE entry_key=?1", params![key,id])?;
                }
                let settings =
                    serde_json::to_string(&entry.recorded_settings).map_err(|_| invalid())?;
                tx.execute("INSERT INTO sync_remote_versions(entry_key,version,recorded_settings,round_to_five) VALUES(?1,?2,?3,?4) ON CONFLICT(entry_key) DO UPDATE SET version=excluded.version,recorded_settings=excluded.recorded_settings,round_to_five=excluded.round_to_five", params![key,change.version,settings,session.details.round_to_five])?;
            } else {
                if let Some(id) = session_id {
                    tx.execute("DELETE FROM sessions WHERE id=?1", [id])?;
                }
                if identity.is_none() {
                    tx.execute("INSERT INTO session_sync_identity(entry_key,session_id,revision) VALUES(?1,NULL,1)", [&key])?;
                }
                tx.execute("INSERT INTO sync_remote_versions(entry_key,version,recorded_settings) VALUES(?1,?2,'null') ON CONFLICT(entry_key) DO UPDATE SET version=excluded.version", params![key,change.version])?;
            }
            tx.execute("INSERT INTO sync_delivered SELECT entry_key,revision FROM session_sync_identity WHERE entry_key=?1 ON CONFLICT(entry_key) DO UPDATE SET revision=excluded.revision", [&key])?;
            RemoteOutcome::Applied
        };
        tx.execute(
            "INSERT INTO sync_remote_receipts VALUES(?1,?2,?3)",
            params![
                token,
                fingerprint,
                serde_json::to_string(&outcome).map_err(|_| invalid())?
            ],
        )?;
        tx.commit()?;
        Ok(outcome)
    }
}
