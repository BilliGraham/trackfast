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
    pub task: Task,
    pub project: Option<ProjectDraft>,
    pub import_token: String,
    pub import_from: String,
}

pub enum Action {
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
