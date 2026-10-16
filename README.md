# Trackfast

Trackfast is a native Rust and egui time tracker for one person on one computer.
Its design is inspired by Toggl Track. It is not a Toggl client and uses no Toggl branding.
All data stays in a local SQLite file. Trackfast has no account, sync, or hosted backend.
The GitHub repository is BilliGraham/trackfast. Keep it private.

## Current scope

- A widget that runs up to three tasks at the same time, each with a play and pause button.
- A Timer screen with the running tasks, a Calendar view, and a List view.
- The Calendar view shows one week on an hour grid, with day totals, a week total, and project shares.
- The List view groups the sessions since the start of last week by day.
- A Projects screen to create, edit, archive, and restore projects with a colour and an optional client.
- A Settings screen with an optional, one-time import from Toggl.

## Widget

Open the widget from the Timer screen. Move the pointer over the widget to show the ⋮ menu at the lower left.

1. Select Add task.
2. Type a new task and select a project, or select a recent task to continue it.
3. The new row starts a session at once.

Select pause to end the session. Select play to start a new session on the same row.
More than one row can run at the same time.
While a row runs, its time shows the current session. While it is paused, its time shows today's total for that task.
Select × to remove a row. A running row stops first, so its time is kept.
The widget holds at most three rows.

In the Calendar view and the List view, each session has a menu with three vertical dots.
Select Continue in widget to add that task to the widget and start it.

Running sessions keep running when you close Trackfast, and they continue when you open it again.
Each start, pause, and remove is one database transaction, so a crash does not lose time.
Sessions shorter than one second are discarded.

## Data

Trackfast keeps its data in `trackfast.db` in the local data folder of the user:

- Windows: `%LOCALAPPDATA%\trackfast\trackfast.db`
- Linux: `~/.local/share/trackfast/trackfast.db`
- macOS: `~/Library/Application Support/trackfast/trackfast.db`

Back up this file to keep your history. Close Trackfast before you copy it.

## Import from Toggl

The import is optional. It copies your Toggl projects, clients, and completed time entries once.

1. Open your [Toggl Track profile](https://track.toggl.com/profile) and copy the API token.
2. In Trackfast, open Settings, paste the token, and enter a start date.
3. Select Import.

Trackfast uses the token only for that import. It does not save the token.
Running entries in Toggl are skipped. A second import adds only entries that are new.
See [the import rules](docs/toggl-import.md).

## Run

Install [rustup](https://rustup.rs) and the native build tools for your platform.
On Linux, also install the libxkbcommon, Wayland, and Mesa OpenGL development packages.
`rust-toolchain.toml` selects Rust 1.98.0. Rustup installs it on the first build.
SQLite is compiled into Trackfast, so no system SQLite library is necessary.

Run this command from the repository root:

```bash
cargo run --locked
```

## Platforms

Linux and Windows are the main development and validation targets.
Keep the design portable to macOS. macOS has lower priority for now.

## Timer model

`src/timer.rs` is a standalone, in-memory timer model from an earlier step. The app does not use it.
See [the timer guide](docs/timer.md).

## Planned work

- A weekly export of time by project and client, for upload to Xero.
- Editing and deleting past sessions.

## Fonts

Trackfast bundles Inter and JetBrains Mono from `assets/fonts/`.
Both fonts use the SIL Open Font License 1.1. The license files are next to the fonts.

## Documentation

- [Architecture](docs/architecture.md)
- [Interface design](docs/interface.md)
- [Toggl import rules](docs/toggl-import.md)
- [Contribution guide and checks](CONTRIBUTING.md)
