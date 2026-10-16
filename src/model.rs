use std::time::{Duration, Instant};

use crate::toggl::Snapshot;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Screen {
    #[default]
    Timer,
    Projects,
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
    pub toggl: Toggl,
}

pub enum Action {
    Navigate(Screen),
    OpenWidget,
    CloseWidget,
    SignIn,
    Refresh,
    SignOut,
}

#[cfg(test)]
mod tests {
    use super::*;

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
