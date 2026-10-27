use super::*;
use crate::entries::Settings;

impl Store {
    pub fn settings(&self) -> Result<Settings> {
        Ok(self.connection.query_row(
            "SELECT round_to_five, weekly_target_hours, glass FROM local_settings WHERE id=1",
            [],
            |row| {
                Ok(Settings {
                    round_to_five: row.get(0)?,
                    weekly_target_hours: row.get(1)?,
                    glass: row.get(2)?,
                })
            },
        )?)
    }
    pub fn save_settings(&mut self, settings: &Settings) -> Result<()> {
        if !settings.weekly_target_hours.is_finite() || settings.weekly_target_hours <= 0.0 {
            return Err(Error("Enter a weekly target greater than zero.".into()));
        }
        let tx = self.connection.transaction()?;
        tx.execute("UPDATE local_settings SET round_to_five=?1, weekly_target_hours=?2, glass=?3 WHERE id=1", params![settings.round_to_five, settings.weekly_target_hours, settings.glass])?;
        tx.commit()?;
        Ok(())
    }
    pub fn save_entry(&mut self, entry: &Session) -> Result<()> {
        if entry.description.trim().is_empty()
            || entry.stop.is_none()
            || entry.details.entry_date.is_none()
            || entry.elapsed(Timestamp::UNIX_EPOCH).as_secs() <= 0
            || entry.elapsed(Timestamp::UNIX_EPOCH).as_secs() > 315_576_000
        {
            return Err(Error(
                "Enter a description and a duration greater than zero and no more than ten years."
                    .into(),
            ));
        }
        if entry.details.duration_seconds.is_none() {
            let zone = entry
                .details
                .time_zone
                .as_deref()
                .and_then(|s| jiff::tz::TimeZone::get(s).ok())
                .ok_or_else(|| Error("Select a valid time zone.".into()))?;
            if entry.start.subsec_nanosecond() != 0
                || entry.stop.is_some_and(|s| s.subsec_nanosecond() != 0)
                || Some(entry.start.to_zoned(zone).date()) != entry.details.entry_date
            {
                return Err(Error("Use whole seconds and the local start date.".into()));
            }
        }
        let tx = self.connection.transaction()?;
        if let Some(project) = entry.project_id {
            let active: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1 AND archived=0)",
                [project],
                |r| r.get(0),
            )?;
            if !active {
                return Err(Error("Select an active project.".into()));
            }
        }
        let round: bool = if entry.id == 0 {
            tx.query_row(
                "SELECT round_to_five FROM local_settings WHERE id=1",
                [],
                |r| r.get(0),
            )?
        } else {
            let old: Option<(bool, bool)> = tx
                .query_row(
                    "SELECT round_to_five, stop_utc IS NULL FROM sessions WHERE id=?1",
                    [entry.id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            match old {
                Some((round, false)) => round,
                _ => return Err(Error("Stop the timer before you edit this entry.".into())),
            }
        };
        tx.execute("INSERT INTO sessions(id,description,project_id,start_utc,stop_utc,duration_seconds,entry_date,time_zone,round_to_five) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(id) DO UPDATE SET description=excluded.description,project_id=excluded.project_id,start_utc=excluded.start_utc,stop_utc=excluded.stop_utc,duration_seconds=excluded.duration_seconds,entry_date=excluded.entry_date,time_zone=excluded.time_zone",
            params![(entry.id != 0).then_some(entry.id),entry.description.trim(),entry.project_id,seconds(entry.start),entry.stop.map(seconds),entry.details.duration_seconds,entry.details.entry_date.map(|d|d.to_string()),entry.details.time_zone,round])?;
        tx.commit()?;
        Ok(())
    }
    pub fn delete_entry(&mut self, id: i64) -> Result<()> {
        let tx = self.connection.transaction()?;
        let running: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sessions WHERE id=?1 AND stop_utc IS NULL)",
            [id],
            |r| r.get(0),
        )?;
        if running {
            return Err(Error("Stop the timer before you delete this entry.".into()));
        }
        tx.execute("DELETE FROM sessions WHERE id=?1", [id])?;
        tx.commit()?;
        Ok(())
    }
}
