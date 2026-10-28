//! The local SQLite store. Only the database worker thread calls this module.

mod local;
#[cfg(test)]
mod sync_identity_tests;
pub mod worker;

use std::path::{Path, PathBuf};

use jiff::Timestamp;
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::entries::{Client, MAX_ROWS, Project, Session, Snapshot, Task, WidgetRow};

/// Each step upgrades the schema by one version. Never edit a released step.
/// Add a new step instead, so that existing files upgrade in place.
const MIGRATIONS: &[&str] = &[
    r"
    CREATE TABLE clients (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL UNIQUE,
        toggl_id INTEGER UNIQUE
    );
    CREATE TABLE projects (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        colour TEXT NOT NULL,
        client_id INTEGER REFERENCES clients(id) ON DELETE SET NULL,
        archived INTEGER NOT NULL DEFAULT 0,
        toggl_id INTEGER UNIQUE
    );
    CREATE TABLE sessions (
        id INTEGER PRIMARY KEY,
        description TEXT NOT NULL,
        project_id INTEGER REFERENCES projects(id) ON DELETE SET NULL,
        start_utc INTEGER NOT NULL,
        stop_utc INTEGER,
        toggl_id INTEGER UNIQUE
    );
    CREATE INDEX sessions_start ON sessions(start_utc);
    CREATE TABLE widget_rows (
        id INTEGER PRIMARY KEY,
        position INTEGER NOT NULL,
        description TEXT NOT NULL,
        project_id INTEGER REFERENCES projects(id) ON DELETE SET NULL,
        session_id INTEGER REFERENCES sessions(id) ON DELETE SET NULL
    );
",
    include_str!("002_local.sql"),
    include_str!("003_sync_identity.sql"),
];

const RECENT_TASKS: i64 = 20;

#[derive(Debug)]
pub struct Error(String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        Self(format!("The local database failed: {error}"))
    }
}

type Result<T> = std::result::Result<T, Error>;

pub fn default_path() -> Option<PathBuf> {
    #[cfg(debug_assertions)]
    if let Some(path) = std::env::var_os("TRACKFAST_REVIEW_DATABASE") {
        return Some(PathBuf::from(path));
    }
    dirs::data_local_dir().map(|dir| dir.join("trackfast").join("trackfast.db"))
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectDraft {
    pub id: Option<i64>,
    pub name: String,
    pub colour: String,
    /// A blank name removes the client.
    pub client: String,
}

/// Toggl data mapped for one import. Ids are Toggl ids.
#[derive(Clone, Debug, Default)]
pub struct Import {
    pub clients: Vec<(i64, String)>,
    pub projects: Vec<ImportedProject>,
    pub sessions: Vec<ImportedSession>,
}

#[derive(Clone, Debug)]
pub struct ImportedProject {
    pub toggl_id: i64,
    pub name: String,
    pub colour: String,
    pub client_toggl_id: Option<i64>,
    pub archived: bool,
}

#[derive(Clone, Debug)]
pub struct ImportedSession {
    pub toggl_id: i64,
    pub description: String,
    pub project_toggl_id: Option<i64>,
    pub start: Timestamp,
    pub stop: Timestamp,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImportCount {
    pub projects: usize,
    pub sessions: usize,
}

pub struct Store {
    connection: Connection,
}

fn seconds(time: Timestamp) -> i64 {
    time.as_second()
}

fn timestamp(seconds: i64) -> rusqlite::Result<Timestamp> {
    Timestamp::from_second(seconds).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Integer, error.into())
    })
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|error| {
                Error(format!(
                    "Trackfast cannot create its data folder {}: {error}",
                    dir.display()
                ))
            })?;
        }
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        Self::prepare(connection)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self> {
        Self::prepare(Connection::open_in_memory()?)
    }

    fn prepare(mut connection: Connection) -> Result<Self> {
        connection.pragma_update(None, "foreign_keys", true)?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        let version = usize::try_from(version).unwrap_or(usize::MAX);
        if version > MIGRATIONS.len() {
            return Err(Error(format!(
                "The database is from a newer Trackfast (schema {version}). Update Trackfast."
            )));
        }
        let transaction = connection.transaction()?;
        for (index, step) in MIGRATIONS.iter().enumerate().skip(version) {
            transaction.execute_batch(step)?;
            transaction.pragma_update(None, "user_version", index as i64 + 1)?;
        }
        transaction.commit()?;
        Ok(Self { connection })
    }

    /// Adds a row for the task and starts a session on it.
    pub fn add_task(&mut self, task: &Task, at: Timestamp) -> Result<()> {
        let transaction = self.connection.transaction()?;
        let rows: i64 =
            transaction.query_row("SELECT COUNT(*) FROM widget_rows", [], |row| row.get(0))?;
        if rows >= MAX_ROWS as i64 {
            return Err(Error(format!(
                "The widget has {MAX_ROWS} tasks. Remove one before adding another."
            )));
        }
        let session = start_session(&transaction, task, at)?;
        transaction.execute(
            "INSERT INTO widget_rows (position, description, project_id, session_id)
             VALUES ((SELECT COALESCE(MAX(position), 0) + 1 FROM widget_rows), ?1, ?2, ?3)",
            params![task.description, task.project_id, session],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn play(&mut self, row: i64, at: Timestamp) -> Result<()> {
        let transaction = self.connection.transaction()?;
        let Some((task, session)) = row_task(&transaction, row)? else {
            return Ok(());
        };
        if session.is_none() {
            let session = start_session(&transaction, &task, at)?;
            transaction.execute(
                "UPDATE widget_rows SET session_id = ?1 WHERE id = ?2",
                params![session, row],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn pause(&mut self, row: i64, at: Timestamp) -> Result<()> {
        let transaction = self.connection.transaction()?;
        stop_row(&transaction, row, at)?;
        transaction.commit()?;
        Ok(())
    }

    /// Stops a running row first, so its time is kept.
    pub fn remove_row(&mut self, row: i64, at: Timestamp) -> Result<()> {
        let transaction = self.connection.transaction()?;
        stop_row(&transaction, row, at)?;
        transaction.execute("DELETE FROM widget_rows WHERE id = ?1", [row])?;
        transaction.commit()?;
        Ok(())
    }

    pub fn save_project(&mut self, draft: &ProjectDraft) -> Result<i64> {
        let name = draft.name.trim();
        if name.is_empty() {
            return Err(Error("Enter a project name.".into()));
        }
        let transaction = self.connection.transaction()?;
        let client = draft.client.trim();
        let client_id: Option<i64> = if client.is_empty() {
            None
        } else {
            transaction.execute(
                "INSERT INTO clients (name) VALUES (?1) ON CONFLICT(name) DO NOTHING",
                [client],
            )?;
            Some(transaction.query_row(
                "SELECT id FROM clients WHERE name = ?1",
                [client],
                |row| row.get(0),
            )?)
        };
        let id = match draft.id {
            Some(id) => {
                transaction.execute(
                    "UPDATE projects SET name = ?1, colour = ?2, client_id = ?3 WHERE id = ?4",
                    params![name, draft.colour, client_id, id],
                )?;
                id
            }
            None => {
                transaction.execute(
                    "INSERT INTO projects (name, colour, client_id) VALUES (?1, ?2, ?3)",
                    params![name, draft.colour, client_id],
                )?;
                transaction.last_insert_rowid()
            }
        };
        transaction.commit()?;
        Ok(id)
    }

    pub fn set_archived(&mut self, project: i64, archived: bool) -> Result<()> {
        self.connection.execute(
            "UPDATE projects SET archived = ?1 WHERE id = ?2",
            params![archived, project],
        )?;
        Ok(())
    }

    /// Adds Toggl data. Rows that an earlier import added stay unchanged.
    pub fn import(&mut self, data: &Import) -> Result<ImportCount> {
        let transaction = self.connection.transaction()?;
        for (toggl_id, name) in &data.clients {
            // A local client with the same name takes the Toggl id.
            transaction.execute(
                "INSERT INTO clients (name, toggl_id) VALUES (?1, ?2)
                 ON CONFLICT(name) DO UPDATE SET toggl_id = COALESCE(toggl_id, excluded.toggl_id)
                 ON CONFLICT(toggl_id) DO NOTHING",
                params![name, toggl_id],
            )?;
        }
        let mut count = ImportCount::default();
        for project in &data.projects {
            count.projects += transaction.execute(
                "INSERT INTO projects (name, colour, client_id, archived, toggl_id)
                 VALUES (?1, ?2, (SELECT id FROM clients WHERE toggl_id = ?3), ?4, ?5)
                 ON CONFLICT(toggl_id) DO NOTHING",
                params![
                    project.name,
                    project.colour,
                    project.client_toggl_id,
                    project.archived,
                    project.toggl_id
                ],
            )?;
        }
        for session in &data.sessions {
            count.sessions += transaction.execute(
                "INSERT INTO sessions (description, project_id, start_utc, stop_utc, toggl_id)
                 VALUES (?1, (SELECT id FROM projects WHERE toggl_id = ?2), ?3, ?4, ?5)
                 ON CONFLICT(toggl_id) DO NOTHING",
                params![
                    session.description,
                    session.project_toggl_id,
                    seconds(session.start),
                    seconds(session.stop),
                    session.toggl_id
                ],
            )?;
        }
        transaction.commit()?;
        Ok(count)
    }

    pub fn snapshot(&self, loaded_from: Timestamp) -> Result<Snapshot> {
        let connection = &self.connection;
        let projects = connection
            .prepare("SELECT id, name, colour, client_id, archived FROM projects ORDER BY name")?
            .query_map([], |row| {
                Ok(Project {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    colour: row.get(2)?,
                    client_id: row.get(3)?,
                    archived: row.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let clients = connection
            .prepare("SELECT id, name FROM clients ORDER BY name")?
            .query_map([], |row| {
                Ok(Client {
                    id: row.get(0)?,
                    name: row.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let sessions = connection
            .prepare(
                "SELECT id, description, project_id, start_utc, stop_utc, duration_seconds, entry_date, time_zone, round_to_five FROM sessions
                 WHERE start_utc >= ?1 OR entry_date >= ?2 OR stop_utc IS NULL ORDER BY start_utc DESC",
            )?
            .query_map(params![seconds(loaded_from),loaded_from.to_zoned(jiff::tz::TimeZone::system()).date().to_string()], read_session)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let rows = connection
            .prepare(
                "SELECT r.id, r.description, r.project_id,
                        s.id, s.description, s.project_id, s.start_utc, s.stop_utc, s.duration_seconds, s.entry_date, s.time_zone, s.round_to_five
                 FROM widget_rows r LEFT JOIN sessions s ON s.id = r.session_id
                 ORDER BY r.position",
            )?
            .query_map([], |row| {
                let session = match row.get::<_, Option<i64>>(3)? {
                    Some(id) => Some(Session {
                        details: read_details(row, 8)?,
                        id,
                        description: row.get(4)?,
                        project_id: row.get(5)?,
                        start: timestamp(row.get(6)?)?,
                        stop: row.get::<_, Option<i64>>(7)?.map(timestamp).transpose()?,
                    }),
                    None => None,
                };
                Ok(WidgetRow {
                    id: row.get(0)?,
                    task: Task {
                        description: row.get(1)?,
                        project_id: row.get(2)?,
                    },
                    session,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let recent_tasks = connection
            .prepare(
                "SELECT description, project_id FROM sessions
                 GROUP BY description, project_id
                 ORDER BY MAX(start_utc) DESC LIMIT ?1",
            )?
            .query_map([RECENT_TASKS], |row| {
                Ok(Task {
                    description: row.get(0)?,
                    project_id: row.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(Snapshot {
            settings: self.settings()?,
            projects,
            clients,
            rows,
            sessions,
            loaded_from,
            recent_tasks,
        })
    }
}

fn read_session(row: &rusqlite::Row) -> rusqlite::Result<Session> {
    Ok(Session {
        details: read_details(row, 5)?,
        id: row.get(0)?,
        description: row.get(1)?,
        project_id: row.get(2)?,
        start: timestamp(row.get(3)?)?,
        stop: row.get::<_, Option<i64>>(4)?.map(timestamp).transpose()?,
    })
}

fn start_session(transaction: &Transaction, task: &Task, at: Timestamp) -> Result<i64> {
    transaction.execute(
        "INSERT INTO sessions (description, project_id, start_utc, round_to_five, entry_date, time_zone) VALUES (?1, ?2, ?3, (SELECT round_to_five FROM local_settings WHERE id=1), ?4, ?5)",
        params![task.description, task.project_id, seconds(at), at.to_zoned(jiff::tz::TimeZone::system()).date().to_string(), jiff::tz::TimeZone::system().iana_name()],
    )?;
    Ok(transaction.last_insert_rowid())
}

fn row_task(transaction: &Transaction, row: i64) -> Result<Option<(Task, Option<i64>)>> {
    Ok(transaction
        .query_row(
            "SELECT description, project_id, session_id FROM widget_rows WHERE id = ?1",
            [row],
            |row| {
                Ok((
                    Task {
                        description: row.get(0)?,
                        project_id: row.get(1)?,
                    },
                    row.get(2)?,
                ))
            },
        )
        .optional()?)
}

/// Ends the row's session. A session under one second has no useful time,
/// so it is deleted instead of kept as an empty entry.
fn stop_row(transaction: &Transaction, row: i64, at: Timestamp) -> Result<()> {
    let Some((_, Some(session))) = row_task(transaction, row)? else {
        return Ok(());
    };
    let start: i64 = transaction.query_row(
        "SELECT start_utc FROM sessions WHERE id = ?1",
        [session],
        |row| row.get(0),
    )?;
    let stop = seconds(at).max(start);
    transaction.execute(
        "UPDATE widget_rows SET session_id = NULL WHERE id = ?1",
        [row],
    )?;
    if stop == start {
        transaction.execute("DELETE FROM sessions WHERE id = ?1", [session])?;
    } else {
        transaction.execute(
            "UPDATE sessions SET stop_utc = ?1 WHERE id = ?2",
            params![stop, session],
        )?;
    }
    Ok(())
}

fn read_details(
    row: &rusqlite::Row,
    offset: usize,
) -> rusqlite::Result<crate::entries::EntryDetails> {
    let date: Option<String> = row.get(offset + 1)?;
    let entry_date = date.map(|s| s.parse()).transpose().map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            offset + 1,
            rusqlite::types::Type::Text,
            Box::new(e),
        )
    })?;
    Ok(crate::entries::EntryDetails {
        duration_seconds: row.get(offset)?,
        entry_date,
        time_zone: row.get(offset + 2)?,
        round_to_five: row.get(offset + 3)?,
    })
}

#[cfg(test)]
mod refinement_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn at(time: &str) -> Timestamp {
        time.parse().unwrap()
    }

    fn task(description: &str) -> Task {
        Task {
            description: description.into(),
            project_id: None,
        }
    }

    fn snapshot(store: &Store) -> Snapshot {
        store.snapshot(at("2026-10-01T00:00:00Z")).unwrap()
    }

    #[test]
    fn migrations_apply_once_and_reject_newer_files() {
        let store = Store::open_in_memory().unwrap();
        let connection = Store::prepare(store.connection).unwrap().connection;
        connection
            .pragma_update(None, "user_version", MIGRATIONS.len() as i64 + 1)
            .unwrap();
        assert!(Store::prepare(connection).is_err());
    }

    #[test]
    fn rows_run_in_parallel_and_pause_keeps_each_session() {
        let mut store = Store::open_in_memory().unwrap();
        store
            .add_task(&task("Review"), at("2026-10-16T08:00:00Z"))
            .unwrap();
        store
            .add_task(&task("Support"), at("2026-10-16T08:10:00Z"))
            .unwrap();
        let data = snapshot(&store);
        assert_eq!(data.running().count(), 2);
        store
            .pause(data.rows[0].id, at("2026-10-16T08:30:00Z"))
            .unwrap();
        store
            .play(data.rows[0].id, at("2026-10-16T09:00:00Z"))
            .unwrap();
        let data = snapshot(&store);
        let review: Vec<_> = data
            .sessions
            .iter()
            .filter(|session| session.description == "Review")
            .collect();
        assert_eq!(review.len(), 2);
        assert_eq!(review[1].stop, Some(at("2026-10-16T08:30:00Z")));
        assert!(data.rows[0].session.as_ref().unwrap().is_running());
        assert!(data.rows[1].session.is_some());
    }

    #[test]
    fn play_on_a_running_row_keeps_the_original_start() {
        let mut store = Store::open_in_memory().unwrap();
        store
            .add_task(&task("Review"), at("2026-10-16T08:00:00Z"))
            .unwrap();
        let row = snapshot(&store).rows[0].id;
        store.play(row, at("2026-10-16T09:00:00Z")).unwrap();
        let data = snapshot(&store);
        assert_eq!(data.sessions.len(), 1);
        assert_eq!(data.sessions[0].start, at("2026-10-16T08:00:00Z"));
    }

    #[test]
    fn removing_a_running_row_keeps_its_time() {
        let mut store = Store::open_in_memory().unwrap();
        store
            .add_task(&task("Review"), at("2026-10-16T08:00:00Z"))
            .unwrap();
        let row = snapshot(&store).rows[0].id;
        store.remove_row(row, at("2026-10-16T08:45:00Z")).unwrap();
        let data = snapshot(&store);
        assert!(data.rows.is_empty());
        assert_eq!(data.sessions[0].stop, Some(at("2026-10-16T08:45:00Z")));
        assert_eq!(data.recent_tasks, [task("Review")]);
    }

    #[test]
    fn sessions_under_a_second_are_discarded() {
        let mut store = Store::open_in_memory().unwrap();
        store
            .add_task(&task("Review"), at("2026-10-16T08:00:00Z"))
            .unwrap();
        let row = snapshot(&store).rows[0].id;
        store.pause(row, at("2026-10-16T08:00:00.400Z")).unwrap();
        assert!(snapshot(&store).sessions.is_empty());
    }

    #[test]
    fn the_widget_holds_at_most_three_rows() {
        let mut store = Store::open_in_memory().unwrap();
        for name in ["A", "B", "C"] {
            store
                .add_task(&task(name), at("2026-10-16T08:00:00Z"))
                .unwrap();
        }
        assert!(
            store
                .add_task(&task("D"), at("2026-10-16T08:00:00Z"))
                .is_err()
        );
        assert_eq!(snapshot(&store).sessions.len(), 3);
    }

    #[test]
    fn a_running_session_survives_a_reopen() {
        let dir = std::env::temp_dir().join(format!("trackfast-test-{}", std::process::id()));
        let path = dir.join("reopen.db");
        let _ = std::fs::remove_file(&path);
        {
            let mut store = Store::open(&path).unwrap();
            store
                .add_task(&task("Review"), at("2026-10-16T08:00:00Z"))
                .unwrap();
        }
        let store = Store::open(&path).unwrap();
        let data = snapshot(&store);
        assert_eq!(data.rows.len(), 1);
        assert_eq!(
            data.rows[0].session.as_ref().unwrap().start,
            at("2026-10-16T08:00:00Z")
        );
        drop(store);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn projects_save_with_a_shared_client() {
        let mut store = Store::open_in_memory().unwrap();
        let mut draft = ProjectDraft {
            id: None,
            name: " Site ".into(),
            colour: "#06aaf5".into(),
            client: "Acme".into(),
        };
        let first = store.save_project(&draft).unwrap();
        draft.name = "Support".into();
        store.save_project(&draft).unwrap();
        draft.id = Some(first);
        draft.name = "Website".into();
        draft.client.clear();
        store.save_project(&draft).unwrap();
        store.set_archived(first, true).unwrap();
        let data = snapshot(&store);
        assert_eq!(data.clients.len(), 1);
        let website = data.project(first).unwrap();
        assert_eq!(website.name, "Website");
        assert_eq!(website.client_id, None);
        assert!(website.archived);
        assert!(store.save_project(&ProjectDraft::default()).is_err());
    }

    #[test]
    fn a_second_import_adds_nothing() {
        let mut store = Store::open_in_memory().unwrap();
        let data = Import {
            clients: vec![(5, "Acme".into())],
            projects: vec![ImportedProject {
                toggl_id: 9,
                name: "Site".into(),
                colour: "#06aaf5".into(),
                client_toggl_id: Some(5),
                archived: false,
            }],
            sessions: vec![ImportedSession {
                toggl_id: 100,
                description: "Review".into(),
                project_toggl_id: Some(9),
                start: at("2026-10-12T08:00:00Z"),
                stop: at("2026-10-12T09:00:00Z"),
            }],
        };
        assert_eq!(
            store.import(&data).unwrap(),
            ImportCount {
                projects: 1,
                sessions: 1
            }
        );
        assert_eq!(store.import(&data).unwrap(), ImportCount::default());
        let snapshot = snapshot(&store);
        let project = &snapshot.projects[0];
        assert_eq!(snapshot.sessions[0].project_id, Some(project.id));
        assert_eq!(
            snapshot.client(project.client_id.unwrap()).unwrap().name,
            "Acme"
        );
    }
}
