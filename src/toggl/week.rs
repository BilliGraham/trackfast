//! Places time entries on a week calendar.

use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp, ToSpan};

use super::{TimeEntry, summary};

pub const MINUTES_PER_DAY: f32 = 24.0 * 60.0;
/// Very short entries still need room for one line of text, so overlap
/// detection treats each block as at least this long.
const MIN_BLOCK_MINUTES: f32 = 15.0;

/// The part of one entry that falls inside one local day.
/// `start` and `end` are minutes after local midnight.
#[derive(Clone, Copy, Debug)]
pub struct Block<'a> {
    pub entry: &'a TimeEntry,
    pub start: f32,
    pub end: f32,
    pub column: usize,
    pub columns: usize,
}

pub fn days(week_start: Date) -> [Date; 7] {
    std::array::from_fn(|offset| {
        week_start
            .checked_add((offset as i64).days())
            .unwrap_or(week_start)
    })
}

pub fn shift(week_start: Date, weeks: i64) -> Date {
    week_start
        .checked_add((weeks * 7).days())
        .unwrap_or(week_start)
}

/// The ISO week number of the week. The middle day decides it, so weeks
/// that start on Sunday or Saturday get the same number as Toggl shows.
pub fn number(week_start: Date) -> i8 {
    week_start
        .checked_add(3.days())
        .unwrap_or(week_start)
        .iso_week_date()
        .week()
}

fn entry_end(entry: &TimeEntry, now: Timestamp) -> Timestamp {
    entry
        .start
        .checked_add(entry.elapsed(now))
        .unwrap_or(entry.start)
}

/// Splits entries at local midnight and places overlapping parts side by side.
pub fn day_blocks<'a>(
    entries: &'a [TimeEntry],
    day: Date,
    tz: &TimeZone,
    now: Timestamp,
) -> Vec<Block<'a>> {
    let Some(day_start) = summary::start_of_day(day, tz) else {
        return Vec::new();
    };
    let day_end = day
        .tomorrow()
        .ok()
        .and_then(|next| summary::start_of_day(next, tz))
        .unwrap_or(day_start + SignedDuration::from_hours(24));
    let minutes = |time: Timestamp| {
        (time.duration_since(day_start).as_secs_f64() / 60.0).clamp(0.0, 1440.0) as f32
    };
    let mut blocks: Vec<Block> = entries
        .iter()
        .filter_map(|entry| {
            let end = entry_end(entry, now);
            let starts_today = entry.start >= day_start && entry.start < day_end;
            let crosses_today = entry.start < day_start && end > day_start;
            (starts_today || crosses_today).then(|| Block {
                entry,
                start: minutes(entry.start),
                end: minutes(end.min(day_end)),
                column: 0,
                columns: 1,
            })
        })
        .collect();
    assign_columns(&mut blocks);
    blocks
}

fn visual_end(block: &Block) -> f32 {
    block.end.max(block.start + MIN_BLOCK_MINUTES)
}

fn assign_columns(blocks: &mut [Block]) {
    blocks.sort_by(|a, b| {
        a.start
            .total_cmp(&b.start)
            .then(visual_end(b).total_cmp(&visual_end(a)))
    });
    let mut group_start = 0;
    let mut group_end = f32::MIN;
    let mut column_ends: Vec<f32> = Vec::new();
    for index in 0..blocks.len() {
        if blocks[index].start >= group_end {
            finish_group(&mut blocks[group_start..index], column_ends.len());
            group_start = index;
            column_ends.clear();
        }
        let end = visual_end(&blocks[index]);
        let column = match column_ends
            .iter()
            .position(|column_end| *column_end <= blocks[index].start)
        {
            Some(free) => free,
            None => {
                column_ends.push(end);
                column_ends.len() - 1
            }
        };
        column_ends[column] = end;
        blocks[index].column = column;
        group_end = group_end.max(end);
    }
    let columns = column_ends.len();
    finish_group(&mut blocks[group_start..], columns);
}

fn finish_group(group: &mut [Block], columns: usize) {
    for block in group {
        block.columns = columns.max(1);
    }
}

/// The total of entries that started in `[from, to)`.
pub fn total_between(
    entries: &[TimeEntry],
    from: Timestamp,
    to: Timestamp,
    now: Timestamp,
) -> SignedDuration {
    project_totals(entries, from, to, now)
        .into_iter()
        .fold(SignedDuration::ZERO, |sum, (_, total)| sum + total)
}

/// Totals per project for entries that started in `[from, to)`, largest first.
pub fn project_totals(
    entries: &[TimeEntry],
    from: Timestamp,
    to: Timestamp,
    now: Timestamp,
) -> Vec<(Option<i64>, SignedDuration)> {
    let mut totals: Vec<(Option<i64>, SignedDuration)> = Vec::new();
    for entry in entries
        .iter()
        .filter(|entry| entry.start >= from && entry.start < to)
    {
        let elapsed = entry.elapsed(now);
        match totals
            .iter_mut()
            .find(|(project, _)| *project == entry.project_id)
        {
            Some((_, total)) => *total += elapsed,
            None => totals.push((entry.project_id, elapsed)),
        }
    }
    totals.sort_by_key(|(_, total)| std::cmp::Reverse(*total));
    totals
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

    const NOW: &str = "2026-10-16T13:00:00Z";

    #[test]
    fn entries_split_at_local_midnight() {
        let tz = TimeZone::fixed(jiff::tz::offset(2));
        // 23:00 to 01:00 local time.
        let entries = [entry("2026-10-12T21:00:00Z", 7200)];
        let now = NOW.parse().unwrap();
        let monday = day_blocks(&entries, date(2026, 10, 12), &tz, now);
        let tuesday = day_blocks(&entries, date(2026, 10, 13), &tz, now);
        assert_eq!((monday[0].start, monday[0].end), (1380.0, 1440.0));
        assert_eq!((tuesday[0].start, tuesday[0].end), (0.0, 60.0));
    }

    #[test]
    fn running_entry_ends_now() {
        let entries = [entry("2026-10-16T11:00:00Z", -1)];
        let blocks = day_blocks(
            &entries,
            date(2026, 10, 16),
            &TimeZone::UTC,
            NOW.parse().unwrap(),
        );
        assert_eq!((blocks[0].start, blocks[0].end), (660.0, 780.0));
    }

    #[test]
    fn overlapping_entries_share_the_width() {
        let entries = [
            entry("2026-10-15T10:00:00Z", 3 * 3600),
            entry("2026-10-15T12:00:00Z", 3600),
            entry("2026-10-15T14:00:00Z", 3600),
            entry("2026-10-15T16:00:00Z", 600),
        ];
        let blocks = day_blocks(
            &entries,
            date(2026, 10, 15),
            &TimeZone::UTC,
            NOW.parse().unwrap(),
        );
        let layout: Vec<_> = blocks
            .iter()
            .map(|block| (block.start, block.column, block.columns))
            .collect();
        assert_eq!(
            layout,
            [(600.0, 0, 2), (720.0, 1, 2), (840.0, 0, 1), (960.0, 0, 1)]
        );
    }

    #[test]
    fn short_entries_reserve_room_for_text() {
        let entries = [
            entry("2026-10-15T10:00:00Z", 60),
            entry("2026-10-15T10:05:00Z", 60),
        ];
        let blocks = day_blocks(
            &entries,
            date(2026, 10, 15),
            &TimeZone::UTC,
            NOW.parse().unwrap(),
        );
        assert_eq!(blocks[0].columns, 2);
        assert_eq!(blocks[1].column, 1);
    }

    #[test]
    fn week_numbers_match_the_middle_day() {
        assert_eq!(number(date(2026, 10, 12)), 42);
        assert_eq!(number(date(2026, 10, 11)), 42);
        assert_eq!(days(date(2026, 10, 12))[6], date(2026, 10, 18));
        assert_eq!(shift(date(2026, 10, 12), -1), date(2026, 10, 5));
    }

    #[test]
    fn project_totals_sum_the_week_largest_first() {
        let mut entries = vec![
            entry("2026-10-12T08:00:00Z", 600),
            entry("2026-10-13T08:00:00Z", 3600),
            entry("2026-10-14T08:00:00Z", 1200),
            entry("2026-10-05T08:00:00Z", 9999),
        ];
        entries[1].project_id = Some(7);
        let totals = project_totals(
            &entries,
            "2026-10-12T00:00:00Z".parse().unwrap(),
            "2026-10-19T00:00:00Z".parse().unwrap(),
            NOW.parse().unwrap(),
        );
        assert_eq!(
            totals,
            [
                (Some(7), SignedDuration::from_secs(3600)),
                (None, SignedDuration::from_secs(1800))
            ]
        );
    }
}
