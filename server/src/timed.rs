use crate::{ApiError, invalid};
use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, Offset};
use chrono_tz::Tz;
use serde_json::{Map, Value};

fn endpoint(value: &Value, zone: Tz) -> Result<DateTime<FixedOffset>, ApiError> {
    let raw = value.as_str().ok_or_else(invalid)?;
    let time = DateTime::parse_from_rfc3339(raw).map_err(|_| invalid())?;
    if time.timestamp_subsec_nanos() != 0
        || raw.ends_with("-00:00")
        || !(1..=9999).contains(&time.year())
        || time.with_timezone(&zone).offset().fix() != *time.offset()
    {
        return Err(invalid());
    }
    Ok(time)
}

pub(crate) fn seconds(fields: &Map<String, Value>, date: NaiveDate) -> Result<i64, ApiError> {
    let zone: Tz = fields["time_zone"]
        .as_str()
        .ok_or_else(invalid)?
        .parse()
        .map_err(|_| invalid())?;
    let start = endpoint(&fields["started_at"], zone)?;
    let stop = endpoint(&fields["stopped_at"], zone)?;
    if start.with_timezone(&zone).date_naive() != date {
        return Err(invalid());
    }
    let seconds = stop.signed_duration_since(start).num_seconds();
    if !(1..=9_007_199_254_740_991).contains(&seconds) {
        return Err(invalid());
    }
    Ok(seconds)
}
