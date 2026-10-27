use crate::{Entry, Error, Timing};
use chrono::{DateTime, Datelike, Offset};
use serde_json::{Value, json};

impl Timing {
    pub(crate) fn validate(&self, entry: &Entry) -> Result<(), Error> {
        let zone: chrono_tz::Tz = self.time_zone.parse().map_err(|_| Error::InvalidEntry)?;
        let endpoint = |raw: &str| {
            let value = DateTime::parse_from_rfc3339(raw).map_err(|_| Error::InvalidEntry)?;
            if value.timestamp_subsec_nanos() != 0
                || raw.ends_with("-00:00")
                || !(1..=9999).contains(&value.year())
                || value.with_timezone(&zone).offset().fix() != *value.offset()
            {
                return Err(Error::InvalidEntry);
            }
            Ok(value)
        };
        let start = endpoint(&self.started_at)?;
        let stop = endpoint(&self.stopped_at)?;
        if start.with_timezone(&zone).date_naive() != entry.entry_date
            || stop.signed_duration_since(start).num_seconds() <= 0
            || stop.signed_duration_since(start).num_seconds() as u64 != entry.original_seconds
        {
            return Err(Error::InvalidEntry);
        }
        Ok(())
    }
}

impl Entry {
    pub(crate) fn request(&self) -> Value {
        let mut value = json!({"entry_id":self.entry_id,"entry_date":self.entry_date,
            "project_id":self.project_id,"description":self.description});
        if let Some(snapshot) = &self.recorded_settings {
            value["recorded_settings"] = json!(snapshot);
        }
        if let Some(timing) = &self.timing {
            value["kind"] = json!("timed");
            value["started_at"] = json!(timing.started_at);
            value["stopped_at"] = json!(timing.stopped_at);
            value["time_zone"] = json!(timing.time_zone);
        } else {
            value["kind"] = json!("duration");
            value["original_seconds"] = json!(self.original_seconds);
        }
        value
    }

    pub(crate) fn from_response(value: &Value) -> Result<Self, Error> {
        let mut entry: Self =
            serde_json::from_value(value.clone()).map_err(|_| Error::InvalidEntry)?;
        if value.get("project_id").is_none() {
            return Err(Error::InvalidEntry);
        }
        entry.timing = match value["kind"].as_str() {
            Some("timed") => {
                Some(serde_json::from_value(value.clone()).map_err(|_| Error::InvalidEntry)?)
            }
            Some("duration")
                if ["started_at", "stopped_at", "time_zone"]
                    .iter()
                    .all(|key| value.get(*key).is_none()) =>
            {
                None
            }
            _ => return Err(Error::InvalidEntry),
        };
        crate::validate(&entry)?;
        Ok(entry)
    }
}
