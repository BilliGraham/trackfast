use std::time::{Duration, Instant};

use jiff::civil::Date;

use crate::toggl::Snapshot;

pub const HOUR_HEIGHTS: [f32; 4] = [36.0, 48.0, 60.0, 84.0];
const DEFAULT_ZOOM: usize = 2;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Screen {
    #[default]
    Timer,
    Projects,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum TimerView {
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

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Connection {
    /// The worker has not read the credential store yet.
    #[default]
    Starting,
    SignedOut,
    SignedIn,
}

#[derive(Default)]
pub struct Toggl {
    pub connection: Connection,
    pub busy: bool,
    pub snapshot: Option<Snapshot>,
    pub problem: Option<String>,
    pub retry_at: Option<Instant>,
}

impl Toggl {
    pub fn request_wait(&self, now: Instant) -> Option<Duration> {
        self.retry_at
            .and_then(|retry_at| retry_at.checked_duration_since(now))
            .filter(|wait| !wait.is_zero())
    }

    pub fn can_refresh(&self, now: Instant) -> bool {
        self.connection == Connection::SignedIn && !self.busy && self.request_wait(now).is_none()
    }

    pub fn can_sign_in(&self, now: Instant) -> bool {
        self.connection == Connection::SignedOut && !self.busy && self.request_wait(now).is_none()
    }
}

#[derive(Default)]
pub struct State {
    pub screen: Screen,
    pub widget_open: bool,
    pub timer_view: TimerView,
    pub calendar: Calendar,
    pub toggl: Toggl,
}

pub enum Action {
    Navigate(Screen),
    OpenWidget,
    CloseWidget,
    SignIn,
    Refresh,
    SignOut,
    ShowTimerView(TimerView),
    /// `None` returns to the current week.
    ShowWeek(Option<Date>),
    LoadWeek,
    Zoom(isize),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_zoom_stays_within_the_steps() {
        let mut calendar = Calendar::default();
        calendar.zoom(-5);
        assert_eq!(calendar.hour_height(), HOUR_HEIGHTS[0]);
        calendar.zoom(9);
        assert_eq!(calendar.hour_height(), HOUR_HEIGHTS[3]);
    }

    #[test]
    fn refresh_waits_until_the_worker_deadline() {
        let start = Instant::now();
        let toggl = Toggl {
            connection: Connection::SignedIn,
            retry_at: Some(start + Duration::from_secs(240)),
            ..Toggl::default()
        };
        assert!(!toggl.can_refresh(start + Duration::from_secs(239)));
        assert!(toggl.can_refresh(start + Duration::from_secs(240)));
        assert!(toggl.can_refresh(start + Duration::from_secs(241)));
    }

    #[test]
    fn refresh_needs_an_idle_signed_in_account() {
        let now = Instant::now();
        let mut toggl = Toggl::default();
        assert!(!toggl.can_refresh(now));
        assert!(!toggl.can_sign_in(now));
        toggl.connection = Connection::SignedIn;
        toggl.busy = true;
        assert!(!toggl.can_refresh(now));
        toggl.connection = Connection::SignedOut;
        assert!(!toggl.can_sign_in(now));
        toggl.busy = false;
        assert!(toggl.can_sign_in(now));
    }

    #[test]
    fn request_deadlines_also_apply_to_sign_in() {
        let now = Instant::now();
        let toggl = Toggl {
            connection: Connection::SignedOut,
            retry_at: Some(now + Duration::from_secs(240)),
            ..Default::default()
        };
        assert!(!toggl.can_sign_in(now));
        assert!(toggl.can_sign_in(now + Duration::from_secs(240)));
    }

    #[test]
    fn refresh_waits_for_the_two_request_budget() {
        let start = Instant::now();
        let toggl = Toggl {
            connection: Connection::SignedIn,
            retry_at: Some(start + Duration::from_secs(240)),
            ..Default::default()
        };
        assert!(!toggl.can_refresh(start + Duration::from_secs(60)));
    }
}
