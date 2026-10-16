use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp, ToSpan};

use super::TimeEntry;

pub struct Day<'a> {
    pub date: Date,
    pub total: SignedDuration,
    pub entries: Vec<&'a TimeEntry>,
}

/// `beginning_of_week` uses Toggl numbering: 0 is Sunday, 1 is Monday.
pub fn week_start(today: Date, beginning_of_week: u8) -> Date {
    let today_offset = i64::from(today.weekday().to_sunday_zero_offset());
    let days_back = (today_offset - i64::from(beginning_of_week.min(6))).rem_euclid(7);
    today.checked_sub(days_back.days()).unwrap_or(today)
}

pub fn start_of_day(date: Date, tz: &TimeZone) -> Option<Timestamp> {
    date.to_zoned(tz.clone())
        .ok()
        .map(|zoned| zoned.timestamp())
}

/// Groups entries by the local date on which they started, newest day first.
pub fn days<'a>(entries: &'a [TimeEntry], tz: &TimeZone, now: Timestamp) -> Vec<Day<'a>> {
    let mut sorted: Vec<&TimeEntry> = entries.iter().collect();
    sorted.sort_by_key(|entry| std::cmp::Reverse(entry.start));
    let mut days: Vec<Day> = Vec::new();
    for entry in sorted {
        let date = entry.start.to_zoned(tz.clone()).date();
        match days.last_mut() {
            Some(day) if day.date == date => {
                day.total += entry.elapsed(now);
                day.entries.push(entry);
            }
            _ => days.push(Day {
                date,
                total: entry.elapsed(now),
                entries: vec![entry],
            }),
        }
    }
    days
}

/// Sums entries that started at or after `since`.
pub fn total_since(entries: &[TimeEntry], since: Timestamp, now: Timestamp) -> SignedDuration {
    entries
        .iter()
        .filter(|entry| entry.start >= since)
        .fold(SignedDuration::ZERO, |total, entry| {
            total + entry.elapsed(now)
        })
}

pub fn clock(duration: SignedDuration) -> String {
    let seconds = duration.as_secs().max(0);
    format!(
        "{}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    fn entry(start: &str, seconds: i64) -> TimeEntry {
        let start: Timestamp = start.parse().unwrap();
        TimeEntry {
            id: start.as_second(),
            project_id: None,
            description: None,
            start,
            stop: (seconds >= 0).then(|| start + SignedDuration::from_secs(seconds)),
            duration: seconds,
        }
    }

    #[test]
    fn week_start_follows_the_user_setting() {
        let friday = date(2026, 10, 16);
        assert_eq!(week_start(friday, 1), date(2026, 10, 12));
        assert_eq!(week_start(friday, 0), date(2026, 10, 11));
        assert_eq!(week_start(friday, 5), friday);
        assert_eq!(week_start(friday, 6), date(2026, 10, 10));
    }

    #[test]
    fn entries_group_by_local_start_date() {
        let tz = TimeZone::fixed(jiff::tz::offset(2));
        let now: Timestamp = "2026-10-13T12:00:00Z".parse().unwrap();
        let entries = [
            entry("2026-10-12T22:30:00Z", 600),
            entry("2026-10-12T21:00:00Z", 1200),
            entry("2026-10-12T08:00:00Z", 60),
        ];
        let days = days(&entries, &tz, now);
        assert_eq!(days.len(), 2);
        assert_eq!(days[0].date, date(2026, 10, 13));
        assert_eq!(days[0].total.as_secs(), 600);
        assert_eq!(days[1].date, date(2026, 10, 12));
        assert_eq!(days[1].entries.len(), 2);
        assert_eq!(days[1].total.as_secs(), 1260);
    }

    #[test]
    fn running_entry_counts_until_now() {
        let now: Timestamp = "2026-10-13T12:00:00Z".parse().unwrap();
        let entries = [
            entry("2026-10-13T11:00:00Z", -1),
            entry("2026-10-13T08:00:00Z", 900),
            entry("2026-10-12T08:00:00Z", 900),
        ];
        let since: Timestamp = "2026-10-13T00:00:00Z".parse().unwrap();
        assert_eq!(total_since(&entries, since, now).as_secs(), 4500);
    }

    #[test]
    fn clock_formats_hours_past_a_day() {
        assert_eq!(clock(SignedDuration::from_secs(93_784)), "26:03:04");
        assert_eq!(clock(SignedDuration::from_secs(-5)), "0:00:00");
    }
}
