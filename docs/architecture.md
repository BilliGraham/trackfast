# Architecture

This document describes the current local app.
See [the roadmap](roadmap.md) and [API contract](api-contract.md) for the target architecture.
Phase 1 does not change the runtime or database schema described here.
The separate phase 3 queue leaves the local time-data flow unchanged. See [the queue design](sync-client.md).
The optional [native account worker](native-account.md) connects sign-in independently of the database worker.

| Module | Responsibility |
| --- | --- |
| `src/main.rs` | Start the native window, select the theme, and install the fonts. |
| `src/account.rs`, `src/account/oauth.rs` | Run native account restoration, Google sign-in, and sign-out outside the interface thread. |
| `src/app.rs` | Apply actions, send database commands, receive events, and manage the widget viewport. |
| `src/model.rs` | Hold interface state, text drafts, and action types. |
| `src/entries/mod.rs` | Define sessions, projects, clients, widget rows, and the snapshot that the views read. |
| `src/entries/summary.rs` | Group sessions by local day and calculate totals. |
| `src/entries/week.rs` | Split sessions at local midnight, place overlapping sessions in lanes, and total projects. |
| `src/db/mod.rs` | Own the SQLite schema, migrations, and every read and write. |
| `src/db/worker.rs` | Run all database calls on one background thread. |
| `src/import.rs` | Run a one-time Toggl import on a background thread. |
| `src/toggl/` | Send read-only Toggl Track API v9 requests and count them. |
| `src/ui/` | Draw the main window, the calendar, the list, projects, settings, and the widget. |
| `src/lib.rs`, `src/timer.rs` | A standalone, in-memory timer model. The app does not use it. |

## Interface flow

1. Views draw the interface and return actions.
2. The application applies the actions after the views return.
3. No view changes application state while it draws.

Text fields are the one exception. egui needs a mutable buffer for text input,
so the application gives the views a `Drafts` value. Entry and settings drafts remain until the worker confirms a successful save.

The widget is a second native viewport in the same process. Both windows read the same `State`.
Closing the widget does not close the main window.
The widget requests the always-on-top window level. Some window managers ignore or restrict that request.
The app repaints after input and after each database event.
While a session runs, the app also repaints once per second to keep the timers current.

## Data flow

1. At start, the app starts the database worker with the path from `db::default_path()`.
2. The worker opens the file, applies migrations, and sends the first `Snapshot`.
3. Each action that changes data becomes one `db::worker::Command`. The app stamps it with the click time.
4. The worker applies the command in one transaction, then sends a new `Snapshot`.
5. If a command fails, the worker sends `Failed` with a message, then the current `Snapshot`.

The interface thread never opens the database file.
A snapshot holds all projects, clients, and widget rows, the sessions from the loaded start time onward, and all running sessions.
Showing an earlier week moves the loaded start time back. The worker then sends a larger snapshot.

## Database

The file is `trackfast.db` in the local data folder of the user. WAL mode and foreign keys are on.

| Table | Content |
| --- | --- |
| `clients` | Client names. Names are unique. |
| `projects` | Name, `#rrggbb` colour, optional client, and archived flag. |
| `sessions` | Description, optional project, start, and stop in UTC seconds. A null stop means the session runs. |
| `local_settings` | Rounding for new entries, weekly target hours, and the visual indicator. |
| `widget_rows` | Up to three tasks, their order, and the running session of each row. |

`clients`, `projects`, and `sessions` have a unique `toggl_id` column for imported rows.

`PRAGMA user_version` records the schema version.
`MIGRATIONS` in `src/db/mod.rs` lists one SQL step for each version. All pending steps run in one transaction.
Never edit a released step. Add a new step, so that existing files upgrade in place.
The app refuses a file with a newer schema version than it knows.

Migration 2 adds optional duration seconds, a saved entry date and time zone, and the applied rounding rule to sessions.
Duration entries use a midnight timestamp as a storage index only. They do not occupy the calendar hour grid.
Existing and imported entries keep exact totals. New entries capture the current rounding rule.
The worker saves manual entries, deletion, and settings in separate transactions.

Pause sets the stop time of the session and clears the session of the row in one transaction.
A session under one second is deleted instead of kept.
Removing a running row stops its session first.

## Toggl import

The import thread fetches Toggl data, maps it to `db::Import`, and sends it to the database worker.
The worker inserts it in one transaction. It skips rows that an earlier import added, by their Toggl ID.
See `docs/toggl-import.md`.

## Dependencies

| Crate | Use |
| --- | --- |
| eframe | Native window and interface with the glow renderer. Default features are off. |
| rusqlite | SQLite access. The `bundled` feature compiles SQLite into the app. |
| dirs | The local data folder of the user. |
| jiff | Time zones, local dates, and RFC 3339 times. |
| ureq | Blocking HTTPS client with rustls, for the import and Google token exchange. |
| serde | Decode Toggl JSON. |
| base64 | Encode the basic authentication header for the import. |
