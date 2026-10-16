use jiff::{SignedDuration, Timestamp};

use crate::toggl::{Project, Snapshot, TimeEntry};

pub const MAX_TASKS: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinAvailability {
    Available,
    AlreadyPinned,
    Full,
}

pub struct PinnedTask {
    pub entry: TimeEntry,
    pub project: Option<Project>,
    pub in_snapshot: bool,
    observed_at: Timestamp,
}

impl PinnedTask {
    fn new(entry: &TimeEntry, snapshot: &Snapshot) -> Self {
        Self {
            entry: entry.clone(),
            project: entry
                .project_id
                .and_then(|id| snapshot.project(id))
                .cloned(),
            in_snapshot: true,
            observed_at: snapshot.fetched_at,
        }
    }

    pub fn elapsed(&self, now: Timestamp) -> SignedDuration {
        // A missing running entry must not continue to accumulate unverified time.
        self.entry.elapsed(if self.in_snapshot {
            now
        } else {
            self.observed_at
        })
    }
}

#[derive(Default)]
pub struct Widget {
    pins: Vec<PinnedTask>,
    initialized: bool,
}

impl Widget {
    pub fn pins(&self) -> &[PinnedTask] {
        &self.pins
    }

    pub fn initialize(&mut self, snapshot: &Snapshot) {
        if self.initialized {
            return;
        }
        self.initialized = true;
        if let Some(entry) = snapshot
            .running()
            .or_else(|| snapshot.entries.iter().max_by_key(|entry| entry.start))
        {
            self.pin(entry.id, snapshot);
        }
    }

    pub fn availability(&self, id: i64) -> PinAvailability {
        if self.pins.iter().any(|pin| pin.entry.id == id) {
            PinAvailability::AlreadyPinned
        } else if self.pins.len() >= MAX_TASKS {
            PinAvailability::Full
        } else {
            PinAvailability::Available
        }
    }

    pub fn pin(&mut self, id: i64, snapshot: &Snapshot) -> bool {
        if self.availability(id) != PinAvailability::Available {
            return false;
        }
        let Some(entry) = snapshot.entries.iter().find(|entry| entry.id == id) else {
            return false;
        };
        self.pins.push(PinnedTask::new(entry, snapshot));
        self.initialized = true;
        true
    }

    pub fn remove(&mut self, id: i64) -> bool {
        let previous_len = self.pins.len();
        self.pins.retain(|pin| pin.entry.id != id);
        self.pins.len() != previous_len
    }

    pub fn refresh(&mut self, snapshot: &Snapshot) {
        for pin in &mut self.pins {
            if let Some(entry) = snapshot
                .entries
                .iter()
                .find(|entry| entry.id == pin.entry.id)
            {
                *pin = PinnedTask::new(entry, snapshot);
            } else {
                pin.in_snapshot = false;
            }
        }
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toggl::{User, Workspace};

    fn snapshot() -> Snapshot {
        let entry = |id, start: &str, duration| TimeEntry {
            id,
            project_id: Some(7),
            description: Some(format!("Task {id}")),
            start: start.parse().unwrap(),
            stop: None,
            duration,
        };
        Snapshot {
            user: User {
                fullname: None,
                beginning_of_week: 1,
            },
            workspaces: vec![Workspace {
                id: 1,
                name: "Test workspace".into(),
            }],
            projects: vec![Project {
                id: 7,
                workspace_id: 1,
                name: "Test project".into(),
                color: Some("#ab87cd".into()),
                active: true,
            }],
            entries: vec![
                entry(1, "2026-10-16T12:00:00Z", 600),
                entry(2, "2026-10-16T11:00:00Z", -1),
                entry(3, "2026-10-16T10:00:00Z", 1200),
                entry(4, "2026-10-16T09:00:00Z", 300),
            ],
            loaded_from: "2026-10-05T00:00:00Z".parse().unwrap(),
            fetched_at: "2026-10-16T13:00:00Z".parse().unwrap(),
        }
    }

    #[test]
    fn first_open_selects_the_running_entry() {
        let mut widget = Widget::default();
        widget.initialize(&snapshot());
        assert_eq!(widget.pins().len(), 1);
        assert_eq!(widget.pins()[0].entry.id, 2);
    }

    #[test]
    fn first_open_selects_the_most_recent_entry_without_a_running_timer() {
        let mut snapshot = snapshot();
        snapshot.entries[1].duration = 600;
        snapshot.entries.reverse();
        let mut widget = Widget::default();
        widget.initialize(&snapshot);
        assert_eq!(widget.pins()[0].entry.id, 1);
    }

    #[test]
    fn empty_history_leaves_an_empty_slot() {
        let mut snapshot = snapshot();
        snapshot.entries.clear();
        let mut widget = Widget::default();
        widget.initialize(&snapshot);
        assert!(widget.pins().is_empty());
    }

    #[test]
    fn explicit_selection_does_not_add_the_default_entry() {
        let snapshot = snapshot();
        let mut widget = Widget::default();
        assert!(widget.pin(3, &snapshot));
        widget.initialize(&snapshot);
        assert_eq!(widget.pins().len(), 1);
        assert_eq!(widget.pins()[0].entry.id, 3);
    }

    #[test]
    fn pins_reject_duplicates_and_a_fourth_entry() {
        let snapshot = snapshot();
        let mut widget = Widget::default();
        for id in 1..=3 {
            assert!(widget.pin(id, &snapshot));
        }
        assert_eq!(widget.availability(2), PinAvailability::AlreadyPinned);
        assert_eq!(widget.availability(4), PinAvailability::Full);
        assert!(!widget.pin(2, &snapshot));
        assert!(!widget.pin(4, &snapshot));
        assert!(!widget.pin(99, &snapshot));
        assert_eq!(widget.pins().len(), 3);
    }

    #[test]
    fn removal_frees_a_slot_and_preserves_the_other_rows() {
        let snapshot = snapshot();
        let mut widget = Widget::default();
        for id in 1..=3 {
            widget.pin(id, &snapshot);
        }
        assert!(widget.remove(2));
        assert_eq!(widget.availability(4), PinAvailability::Available);
        assert!(widget.pin(4, &snapshot));
        assert_eq!(
            widget
                .pins()
                .iter()
                .map(|pin| pin.entry.id)
                .collect::<Vec<_>>(),
            vec![1, 3, 4]
        );
        assert!(snapshot.entries[1].is_running());
    }

    #[test]
    fn refresh_updates_selected_entries_without_replacing_or_reordering_them() {
        let mut snapshot = snapshot();
        let mut widget = Widget::default();
        widget.pin(2, &snapshot);
        widget.pin(1, &snapshot);
        snapshot.entries[1].description = Some("Updated task".into());
        snapshot.entries[1].duration = 1800;
        snapshot.entries[1].stop = Some("2026-10-16T11:30:00Z".parse().unwrap());
        snapshot.projects[0].name = "Renamed project".into();
        snapshot.entries.reverse();
        widget.refresh(&snapshot);
        assert_eq!(
            widget
                .pins()
                .iter()
                .map(|pin| pin.entry.id)
                .collect::<Vec<_>>(),
            vec![2, 1]
        );
        assert_eq!(
            widget.pins()[0].entry.description.as_deref(),
            Some("Updated task")
        );
        assert_eq!(
            widget.pins()[0].project.as_ref().unwrap().name,
            "Renamed project"
        );
        assert!(!widget.pins()[0].entry.is_running());
    }

    #[test]
    fn missing_entries_keep_the_last_observed_time() {
        let mut snapshot = snapshot();
        let mut widget = Widget::default();
        widget.pin(2, &snapshot);
        snapshot.entries.clear();
        widget.refresh(&snapshot);
        let pin = &widget.pins()[0];
        assert!(!pin.in_snapshot);
        assert_eq!(
            pin.elapsed("2026-10-16T20:00:00Z".parse().unwrap())
                .as_secs(),
            7200
        );
    }

    #[test]
    fn removing_the_last_pin_does_not_restore_a_default_on_reopen() {
        let snapshot = snapshot();
        let mut widget = Widget::default();
        widget.initialize(&snapshot);
        widget.remove(2);
        widget.initialize(&snapshot);
        assert!(widget.pins().is_empty());
    }

    #[test]
    fn clearing_the_widget_removes_account_data_and_allows_a_new_default() {
        let mut widget = Widget::default();
        widget.initialize(&snapshot());
        widget.clear();
        assert!(widget.pins().is_empty());
        let mut next = snapshot();
        next.entries.retain(|entry| entry.id == 4);
        widget.initialize(&next);
        assert_eq!(widget.pins()[0].entry.id, 4);
    }

    fn signed_in_state() -> crate::model::State {
        crate::model::State {
            toggl: crate::model::Toggl {
                connection: crate::model::Connection::SignedIn,
                snapshot: Some(snapshot()),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn list_selection_opens_the_widget_and_enforces_the_limit() {
        let mut state = signed_in_state();
        assert!(state.pin_entry(3));
        assert!(state.widget_open);
        assert_eq!(state.widget.pins().len(), 1);
        assert_eq!(state.widget.pins()[0].entry.id, 3);
        assert!(state.pin_entry(1));
        assert!(state.pin_entry(2));
        assert!(!state.pin_entry(4));
        assert_eq!(state.widget.pins().len(), 3);
    }

    #[test]
    fn reopen_and_refresh_preserve_the_selection() {
        let mut state = signed_in_state();
        state.open_widget();
        state.widget_open = false;
        state
            .toggl
            .snapshot
            .as_mut()
            .unwrap()
            .entries
            .retain(|entry| entry.id != 2);
        state.refresh_widget();
        state.open_widget();
        assert!(state.widget_open);
        assert_eq!(state.widget.pins().len(), 1);
        assert_eq!(state.widget.pins()[0].entry.id, 2);
        assert!(!state.widget.pins()[0].in_snapshot);
    }

    #[test]
    fn account_reset_closes_the_widget_and_discards_its_rows() {
        let mut state = signed_in_state();
        state.open_widget();
        state.clear_widget();
        assert!(!state.widget_open);
        assert!(state.widget.pins().is_empty());
        state.toggl.connection = crate::model::Connection::SignedOut;
        assert!(!state.pin_entry(1));
        state.open_widget();
        assert!(!state.widget_open);
    }
}
