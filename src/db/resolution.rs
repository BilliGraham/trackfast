use super::*;
use trackfast_sync::{
    Entry, Scope,
    native_resolution::{Choice, Resolution},
    transfer::{Change, Transfer},
};
use uuid::Uuid;

impl Store {
    pub fn apply_resolution(
        &mut self,
        scope: &Scope,
        operation: Uuid,
        resolution: &Resolution,
    ) -> Result<()> {
        if resolution.choice == Choice::Local {
            return self.confirm_delivery(scope, operation);
        }
        let tx = self.connection.transaction()?;
        enrollment::require_scope(&tx, scope)?;
        let done: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sync_applied_resolutions WHERE operation_id=?1)",
            [operation.to_string()],
            |r| r.get(0),
        )?;
        if done {
            return Ok(());
        }
        let (key, frozen_revision, body): (String, i64, String) = tx.query_row(
            "SELECT entry_key,revision,body FROM sync_outbox WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let transfer: Transfer = serde_json::from_str(&body)
            .map_err(|_| Error("The pending transfer cannot be read.".into()))?;
        if transfer.operation_id != operation
            || transfer.entry_id != resolution.remote.entry_id
            || resolution.remote.version <= 0
        {
            return Err(Error(
                "The conflict result does not match the pending transfer.".into(),
            ));
        }
        let (current_revision, session_id): (i64, Option<i64>) = tx.query_row(
            "SELECT revision,session_id FROM session_sync_identity WHERE entry_key=?1",
            [&key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let current = session_id.map(|id| tx.query_row("SELECT id,description,project_id,start_utc,stop_utc,duration_seconds,entry_date,time_zone,round_to_five FROM sessions WHERE id=?1", [id], read_session)).transpose()?;
        if let Some(local) = &current {
            if resolution.choice == Choice::Copy || current_revision != frozen_revision {
                write_session(&tx, local, None)?;
            }
        } else if resolution.choice == Choice::Copy {
            let local = match &transfer.change {
                Change::Create(e) | Change::Replace(e) => e,
                Change::Delete => return Err(Error("A deletion cannot be copied.".into())),
            };
            write_session(&tx, &from_api(&tx, local, None)?, None)?;
        }
        let later_deletion = session_id.is_none()
            && current_revision != frozen_revision
            && resolution.remote.entry.is_some();
        if later_deletion {
            tx.execute("INSERT INTO sync_delivered(entry_key,revision) VALUES(?1,?2) ON CONFLICT(entry_key) DO UPDATE SET revision=excluded.revision", params![key,frozen_revision])?;
        } else if let Some(remote) = &resolution.remote.entry {
            if remote.entry_id != transfer.entry_id {
                return Err(Error("The API entry does not match this conflict.".into()));
            }
            let entry = from_api(&tx, remote, current.as_ref().and_then(|s| s.project_id))?;
            let id = write_session(&tx, &entry, session_id)?;
            if session_id.is_none() {
                tx.execute(
                    "DELETE FROM session_sync_identity WHERE session_id=?1",
                    [id],
                )?;
                tx.execute("UPDATE session_sync_identity SET session_id=?2,revision=revision+1 WHERE entry_key=?1", params![key,id])?;
            }
        } else if let Some(id) = session_id {
            tx.execute("DELETE FROM sessions WHERE id=?1", [id])?;
        }
        if !later_deletion {
            tx.execute("INSERT INTO sync_delivered(entry_key,revision) SELECT entry_key,revision FROM session_sync_identity WHERE entry_key=?1 ON CONFLICT(entry_key) DO UPDATE SET revision=excluded.revision", [&key])?;
        }
        tx.execute("DELETE FROM sync_outbox WHERE id=1", [])?;
        tx.execute(
            "INSERT INTO sync_applied_resolutions VALUES(?1)",
            [operation.to_string()],
        )?;
        tx.commit()?;
        Ok(())
    }
}

fn from_api(tx: &Transaction<'_>, entry: &Entry, preferred: Option<i64>) -> Result<Session> {
    trackfast_sync::validate(entry)
        .map_err(|_| Error("The API entry failed validation.".into()))?;
    let project_id = if let Some(remote) = entry.project_id {
        let mut query = tx.prepare("SELECT local_project_id FROM sync_project_mappings WHERE remote_project_id=?1 ORDER BY local_project_id")?;
        let ids = query
            .query_map([remote.to_string()], |r| r.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if preferred.is_some_and(|id| ids.contains(&id)) {
            preferred
        } else if ids.len() == 1 {
            Some(ids[0])
        } else {
            return Err(Error(
                "Map the API project to one local project in Settings, then start delivery again."
                    .into(),
            ));
        }
    } else {
        None
    };
    let date: jiff::civil::Date = entry
        .entry_date
        .to_string()
        .parse()
        .map_err(|_| Error("The API date is invalid.".into()))?;
    let (start, stop, time_zone, duration) = if let Some(timing) = &entry.timing {
        (
            timing
                .started_at
                .parse::<Timestamp>()
                .map_err(|_| Error("The API start time is invalid.".into()))?,
            timing
                .stopped_at
                .parse::<Timestamp>()
                .map_err(|_| Error("The API stop time is invalid.".into()))?,
            Some(timing.time_zone.clone()),
            None,
        )
    } else {
        let at = date
            .at(0, 0, 0, 0)
            .to_zoned(jiff::tz::TimeZone::UTC)
            .map_err(|_| Error("The API date is invalid.".into()))?
            .timestamp();
        (
            at,
            at,
            None,
            Some(
                i64::try_from(entry.original_seconds)
                    .map_err(|_| Error("The API duration is invalid.".into()))?,
            ),
        )
    };
    Ok(Session {
        id: 0,
        description: entry.description.clone(),
        project_id,
        start,
        stop: Some(stop),
        details: crate::entries::EntryDetails {
            duration_seconds: duration,
            entry_date: Some(date),
            time_zone,
            round_to_five: entry
                .recorded_settings
                .as_ref()
                .and_then(|s| s.rounding_interval_minutes)
                == Some(5),
        },
    })
}

fn write_session(tx: &Transaction<'_>, entry: &Session, id: Option<i64>) -> Result<i64> {
    tx.execute("INSERT INTO sessions(id,description,project_id,start_utc,stop_utc,duration_seconds,entry_date,time_zone,round_to_five) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(id) DO UPDATE SET description=excluded.description,project_id=excluded.project_id,start_utc=excluded.start_utc,stop_utc=excluded.stop_utc,duration_seconds=excluded.duration_seconds,entry_date=excluded.entry_date,time_zone=excluded.time_zone,round_to_five=excluded.round_to_five", params![id,entry.description,entry.project_id,seconds(entry.start),entry.stop.map(seconds),entry.details.duration_seconds,entry.details.entry_date.map(|d|d.to_string()),entry.details.time_zone,entry.details.round_to_five])?;
    Ok(id.unwrap_or_else(|| tx.last_insert_rowid()))
}
