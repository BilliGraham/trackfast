use jiff::civil::Date;

use crate::db::ProjectDraft;
use crate::entries::{Snapshot, Task};

pub const HOUR_HEIGHTS: [f32; 4] = [36.0, 48.0, 60.0, 84.0];
const DEFAULT_ZOOM: usize = 2;

/// Colours offered for new projects, in `#rrggbb` form.
pub const PALETTE: [&str; 10] = [
    "#06aaf5", "#c56bff", "#ea468d", "#fb8b14", "#c7af14", "#4dc3a0", "#2da608", "#525266",
    "#d94182", "#0b83d9",
];

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Screen {
    #[default]
    Timer,
    Projects,
    Settings,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum TimerView {
    DayBlocks,
    #[default]
    Calendar,
    List,
}

pub struct Calendar {
    /// `None` shows the current week.
    pub week: Option<Date>,
    pub zoom: usize,
}

impl Default for Calendar {
    fn default() -> Self {
        Self {
            week: None,
            zoom: DEFAULT_ZOOM,
        }
    }
}

impl Calendar {
    pub fn hour_height(&self) -> f32 {
        HOUR_HEIGHTS[self.zoom.min(HOUR_HEIGHTS.len() - 1)]
    }

    pub fn zoom(&mut self, step: isize) {
        self.zoom = self
            .zoom
            .saturating_add_signed(step)
            .min(HOUR_HEIGHTS.len() - 1);
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
pub enum ImportState {
    #[default]
    Idle,
    Running(String),
    Done(String),
    Failed(String),
}

#[derive(Default)]
pub struct State {
    pub saving: bool,
    pub notice: Option<String>,
    pub screen: Screen,
    pub widget_open: bool,
    pub timer_view: TimerView,
    pub calendar: Calendar,
    /// `None` until the database worker sends its first snapshot.
    pub data: Option<Snapshot>,
    pub problem: Option<String>,
    pub import: ImportState,
}

/// Text that the views edit in place, because egui text fields need a
/// mutable buffer. The app reads and clears a draft when it acts on it.
#[derive(Default)]
pub struct Drafts {
    pub entry: Option<EntryDraft>,
    pub settings: Option<crate::entries::Settings>,
    pub delete_entry: Option<i64>,
    pub task: Task,
    pub project: Option<ProjectDraft>,
    pub import_token: String,
    pub import_from: String,
}

pub enum Action {
    DraftEntry(Box<EntryDraft>),
    StartEntry,
    EditEntry(Option<i64>),
    CancelEntry,
    SaveEntry,
    DeleteEntry(i64),
    SaveSettings,
    Navigate(Screen),
    OpenWidget,
    CloseWidget,
    ShowTimerView(TimerView),
    /// `None` returns to the current week.
    ShowWeek(Option<Date>),
    Zoom(isize),
    /// Adds a widget row for the task and starts it.
    AddTask(Task),
    /// Adds the task in `Drafts::task`.
    AddDraftTask,
    Play(i64),
    Pause(i64),
    RemoveRow(i64),
    /// `None` starts a new project.
    EditProject(Option<i64>),
    CancelProject,
    SaveProject,
    SetArchived(i64, bool),
    StartImport,
}

impl ProjectDraft {
    pub fn for_project(snapshot: &Snapshot, id: Option<i64>) -> Self {
        let Some(project) = id.and_then(|id| snapshot.project(id)) else {
            let colour = PALETTE[snapshot.projects.len() % PALETTE.len()];
            return Self {
                colour: colour.into(),
                ..Self::default()
            };
        };
        Self {
            id: Some(project.id),
            name: project.name.clone(),
            colour: project.colour.clone(),
            client: project
                .client_id
                .and_then(|id| snapshot.client(id))
                .map(|client| client.name.clone())
                .unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entries::{Client, Project};

    #[test]
    fn calendar_zoom_stays_within_the_steps() {
        let mut calendar = Calendar::default();
        calendar.zoom(-5);
        assert_eq!(calendar.hour_height(), HOUR_HEIGHTS[0]);
        calendar.zoom(9);
        assert_eq!(calendar.hour_height(), HOUR_HEIGHTS[3]);
    }

    #[test]
    fn project_drafts_copy_the_project_or_pick_the_next_colour() {
        let snapshot = Snapshot {
            projects: vec![Project {
                id: 3,
                name: "Site".into(),
                colour: "#123456".into(),
                client_id: Some(5),
                archived: false,
            }],
            clients: vec![Client {
                id: 5,
                name: "Acme".into(),
            }],
            ..Default::default()
        };
        let existing = ProjectDraft::for_project(&snapshot, Some(3));
        assert_eq!(existing.client, "Acme");
        assert_eq!(existing.colour, "#123456");
        let new = ProjectDraft::for_project(&snapshot, None);
        assert_eq!(new.id, None);
        assert_eq!(new.colour, PALETTE[1]);
    }
}

#[cfg(test)]
mod entry_form_tests {
    use super::*;
    #[test]
    fn form_accepts_exact_duration_and_timed_work_without_splitting() {
        let now = "2026-10-26T09:03:00Z".parse().unwrap();
        let mut form = EntryDraft::new(now, &jiff::tz::TimeZone::UTC);
        form.description = "Design".into();
        form.duration = "3:00:01".into();
        assert_eq!(form.session().unwrap().elapsed(now).as_secs(), 10801);
        form.timed = true;
        form.start = "23:50:00".into();
        form.end_date = "2026-10-27".into();
        form.stop = "00:10:00".into();
        let entry = form.session().unwrap();
        assert_eq!(entry.elapsed(now).as_secs(), 1200);
        assert_eq!(entry.details.entry_date.unwrap().to_string(), "2026-10-26");
        form.end_date = "2026-10-26".into();
        assert!(form.session().is_err());
    }
    #[test]
    fn form_rejects_ambiguous_local_times_and_bad_durations() {
        let mut form = EntryDraft::new(
            "2026-11-01T12:00:00Z".parse().unwrap(),
            &jiff::tz::TimeZone::UTC,
        );
        form.description = "Review".into();
        for duration in ["0:00", "-1:00", "1:60", "1.5", "1:01:60"] {
            form.duration = duration.into();
            assert!(form.session().is_err());
        }
        form.timed = true;
        form.time_zone = "America/New_York".into();
        form.start = "01:30".into();
        form.stop = "03:30".into();
        assert!(form.session().is_err());
    }
}

#[derive(Clone)]
pub struct EntryDraft {
    pub focus_description: bool,
    pub id: i64,
    pub description: String,
    pub project_id: Option<i64>,
    pub timed: bool,
    pub date: String,
    pub end_date: String,
    pub start: String,
    pub stop: String,
    pub duration: String,
    pub time_zone: String,
    pub round_to_five: Option<bool>,
}
impl EntryDraft {
    pub fn calendar_range(day: Date, a: u16, b: u16, tz: &jiff::tz::TimeZone) -> Option<Self> {
        let (start, end) = (a.min(b), a.max(b));
        if start == end || end > 1440 {
            return None;
        }
        let at = day.to_zoned(tz.clone()).ok()?.timestamp();
        let mut form = Self::new(at, tz);
        form.timed = true;
        form.date = day.to_string();
        form.start = format!("{:02}:{:02}:00", start / 60, start % 60);
        form.end_date = if end == 1440 {
            day.tomorrow().ok()?
        } else {
            day
        }
        .to_string();
        form.stop = format!("{:02}:{:02}:00", end % 1440 / 60, end % 60);
        Some(form)
    }

    pub fn new(now: jiff::Timestamp, tz: &jiff::tz::TimeZone) -> Self {
        let local = now.to_zoned(tz.clone());
        Self {
            focus_description: true,
            id: 0,
            description: String::new(),
            project_id: None,
            timed: false,
            date: local.date().to_string(),
            end_date: local.date().to_string(),
            start: local.time().strftime("%H:%M:%S").to_string(),
            stop: local.time().strftime("%H:%M:%S").to_string(),
            duration: "1:00:00".into(),
            time_zone: tz.iana_name().unwrap_or("UTC").into(),
            round_to_five: None,
        }
    }
    pub fn from_session(entry: &crate::entries::Session, tz: &jiff::tz::TimeZone) -> Self {
        let zone = entry
            .details
            .time_zone
            .as_deref()
            .and_then(|name| jiff::tz::TimeZone::get(name).ok())
            .unwrap_or_else(|| tz.clone());
        let mut form = Self::new(entry.start, &zone);
        form.id = entry.id;
        form.description = entry.description.clone();
        form.project_id = entry.project_id;
        form.timed = entry.details.duration_seconds.is_none();
        form.date = entry.date(&zone).to_string();
        let stop = entry.stop.unwrap_or(entry.start).to_zoned(zone);
        form.end_date = stop.date().to_string();
        form.stop = stop.time().strftime("%H:%M:%S").to_string();
        form.duration = crate::entries::summary::clock(entry.elapsed(entry.start));
        form.round_to_five = Some(entry.details.round_to_five);
        form
    }
    pub fn session(&self) -> Result<crate::entries::Session, String> {
        use crate::entries::{EntryDetails, Session};
        let fail = |message: &str| message.to_owned();
        if self.description.trim().is_empty() {
            return Err(fail("Enter a description."));
        }
        let date: Date = self
            .date
            .parse()
            .map_err(|_| fail("Enter the date as YYYY-MM-DD."))?;
        let zone = jiff::tz::TimeZone::get(&self.time_zone)
            .map_err(|_| fail("Enter an IANA time zone, such as Africa/Johannesburg."))?;
        let (start, stop, duration) = if self.timed {
            let parse = |date: &str, time: &str| -> Result<jiff::Timestamp, String> {
                let dt = format!("{date}T{time}")
                    .parse()
                    .map_err(|_| fail("Enter valid dates and times. Use HH:MM or HH:MM:SS."))?;
                zone.to_ambiguous_timestamp(dt).unambiguous().map_err(|_|fail("This local time is repeated or skipped by a clock change. Select an unambiguous time."))
            };
            let start = parse(&self.date, &self.start)?;
            let stop = parse(&self.end_date, &self.stop)?;
            if stop <= start {
                return Err(fail("The stop must be later than the start."));
            }
            (start, stop, None)
        } else {
            let parts: Vec<_> = self.duration.split(':').collect();
            if !(2..=3).contains(&parts.len())
                || parts
                    .iter()
                    .any(|p| p.is_empty() || !p.bytes().all(|c| c.is_ascii_digit()))
            {
                return Err(fail("Enter a duration as H:MM or H:MM:SS."));
            }
            let nums = parts
                .iter()
                .map(|p| p.parse::<i64>())
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| fail("The duration is too large."))?;
            if nums[1] >= 60 || nums.get(2).is_some_and(|n| *n >= 60) {
                return Err(fail("Minutes and seconds must be less than 60."));
            }
            let seconds = nums[0]
                .checked_mul(3600)
                .and_then(|n| n.checked_add(nums[1] * 60 + nums.get(2).copied().unwrap_or(0)))
                .filter(|n| *n > 0 && *n <= 315_576_000)
                .ok_or_else(|| {
                    fail("Enter a duration greater than zero and no more than ten years.")
                })?;
            let anchor = date
                .to_zoned(zone.clone())
                .map_err(|_| fail("This date is unavailable in the selected time zone."))?
                .timestamp();
            (anchor, anchor, Some(seconds))
        };
        Ok(Session {
            id: self.id,
            description: self.description.trim().into(),
            project_id: self.project_id,
            start,
            stop: Some(stop),
            details: EntryDetails {
                duration_seconds: duration,
                entry_date: Some(date),
                time_zone: Some(self.time_zone.clone()),
                round_to_five: self.round_to_five.unwrap_or(false),
            },
        })
    }
}

#[cfg(test)]
mod selection_tests {
    use super::*;
    #[test]
    fn calendar_selection_normalizes_direction_and_keeps_midnight() {
        let mut form = EntryDraft::calendar_range(
            "2026-10-27".parse().unwrap(),
            600,
            540,
            &jiff::tz::TimeZone::UTC,
        )
        .unwrap();
        form.description = "Review".into();
        assert_eq!(
            form.session()
                .unwrap()
                .elapsed(jiff::Timestamp::UNIX_EPOCH)
                .as_secs(),
            3600
        );
        let form = EntryDraft::calendar_range(
            "2026-10-27".parse().unwrap(),
            1435,
            1440,
            &jiff::tz::TimeZone::UTC,
        )
        .unwrap();
        assert_eq!(form.end_date, "2026-10-28");
        assert_eq!(form.stop, "00:00:00");
        assert!(
            EntryDraft::calendar_range(
                "2026-10-27".parse().unwrap(),
                60,
                60,
                &jiff::tz::TimeZone::UTC
            )
            .is_none()
        );
    }
}
