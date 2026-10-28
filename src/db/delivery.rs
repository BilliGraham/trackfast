use super::*;
use trackfast_sync::{
    Entry, RecordedSettings, Scope, Timing,
    transfer::{Change, Transfer},
};
use uuid::Uuid;

fn invalid() -> Error {
    Error("This entry cannot be sent. Check its date, time zone, duration, and description.".into())
}

impl Store {
    pub fn prepare_delivery(&mut self, scope: &Scope) -> Result<Option<Transfer>> {
        let tx = self.connection.transaction()?;
        enrollment::require_scope(&tx, scope)?;
        let pending: Option<String> = tx
            .query_row("SELECT body FROM sync_outbox WHERE id=1", [], |r| r.get(0))
            .optional()?;
        if let Some(body) = pending {
            return serde_json::from_str(&body).map(Some).map_err(|_| invalid());
        }
        let candidate: Option<(String,i64,Option<i64>,i64)>=tx.query_row(
            "SELECT i.entry_key,i.revision,i.session_id,COALESCE(d.revision,0) FROM session_sync_identity i
             LEFT JOIN sessions s ON s.id=i.session_id LEFT JOIN sync_delivered d ON d.entry_key=i.entry_key
             LEFT JOIN sync_project_mappings m ON m.local_project_id=s.project_id
             WHERE i.sequence>(SELECT boundary FROM sync_enrollment WHERE id=1) AND i.revision>COALESCE(d.revision,0)
             AND ((i.session_id IS NULL AND d.revision>0) OR (s.stop_utc IS NOT NULL AND s.toggl_id IS NULL AND (s.project_id IS NULL OR m.remote_project_id IS NOT NULL)))
             ORDER BY i.sequence LIMIT 1",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        let Some((key, revision, session_id, confirmed)) = candidate else {
            return Ok(None);
        };
        let entry_id = Uuid::parse_str(&key).map_err(|_| invalid())?;
        let change = if let Some(id) = session_id {
            let session=tx.query_row("SELECT id,description,project_id,start_utc,stop_utc,duration_seconds,entry_date,time_zone,round_to_five FROM sessions WHERE id=?1",[id],read_session)?;
            let project_id = if let Some(project) = session.project_id {
                let remote: String = tx.query_row(
                    "SELECT remote_project_id FROM sync_project_mappings WHERE local_project_id=?1",
                    [project],
                    |r| r.get(0),
                )?;
                Some(Uuid::parse_str(&remote).map_err(|_| invalid())?)
            } else {
                None
            };
            let timing = if session.details.duration_seconds.is_none() {
                let zone = session.details.time_zone.clone().ok_or_else(invalid)?;
                let time_zone = jiff::tz::TimeZone::get(&zone).map_err(|_| invalid())?;
                Some(Timing {
                    started_at: session
                        .start
                        .to_zoned(time_zone.clone())
                        .strftime("%Y-%m-%dT%H:%M:%S%:z")
                        .to_string(),
                    stopped_at: session
                        .stop
                        .ok_or_else(invalid)?
                        .to_zoned(time_zone)
                        .strftime("%Y-%m-%dT%H:%M:%S%:z")
                        .to_string(),
                    time_zone: zone,
                })
            } else {
                None
            };
            let entry = Entry {
                entry_id,
                project_id,
                timing,
                entry_date: session
                    .details
                    .entry_date
                    .ok_or_else(invalid)?
                    .to_string()
                    .parse()
                    .map_err(|_| invalid())?,
                description: session.description.clone(),
                original_seconds: u64::try_from(session.elapsed(Timestamp::UNIX_EPOCH).as_secs())
                    .map_err(|_| invalid())?,
                recorded_settings: Some(RecordedSettings {
                    rounding_interval_minutes: session.details.round_to_five.then_some(5),
                }),
            };
            trackfast_sync::validate(&entry).map_err(|_| invalid())?;
            if confirmed == 0 {
                Change::Create(entry)
            } else {
                Change::Replace(entry)
            }
        } else {
            Change::Delete
        };
        let transfer = Transfer {
            operation_id: Uuid::new_v4(),
            entry_id,
            change,
        };
        tx.execute(
            "INSERT INTO sync_outbox VALUES(1,?1,?2,?3)",
            params![
                key,
                revision,
                serde_json::to_string(&transfer).map_err(|_| invalid())?
            ],
        )?;
        tx.commit()?;
        Ok(Some(transfer))
    }

    pub fn confirm_delivery(&mut self, scope: &Scope, operation: Uuid) -> Result<()> {
        let tx = self.connection.transaction()?;
        enrollment::require_scope(&tx, scope)?;
        let pending: Option<(String, i64, String)> = tx
            .query_row(
                "SELECT entry_key,revision,body FROM sync_outbox WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let (key, revision, body) =
            pending.ok_or_else(|| Error("The pending delivery is unavailable.".into()))?;
        let transfer: Transfer = serde_json::from_str(&body).map_err(|_| invalid())?;
        if transfer.operation_id != operation {
            return Err(Error(
                "The delivery receipt does not match the pending operation.".into(),
            ));
        }
        tx.execute("INSERT INTO sync_delivered VALUES(?1,?2) ON CONFLICT(entry_key) DO UPDATE SET revision=excluded.revision",params![key,revision])?;
        tx.execute("DELETE FROM sync_outbox WHERE id=1", [])?;
        tx.commit()?;
        Ok(())
    }
}
