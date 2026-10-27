# Trackfast

Trackfast is a native Rust and egui time tracker for one person on one computer.
Its design is inspired by Toggl Track. It is not a Toggl client and uses no Toggl branding.
The native app stores its data in local SQLite. An optional [pilot account connection](docs/native-account.md) supports Google sign-in to the local API.
The target product has a shared API and a polished native reference client.
Phase 1 defines [the API contract](docs/api-contract.md) and [delivery roadmap](docs/roadmap.md).
The app remains local while the API develops. Internal teams can later build other clients against the same API.
The GitHub repository is BilliGraham/trackfast. The user selected public visibility.
The separate [development API](docs/server.md) stores completed timed and duration-only entries in Postgres.
It supports local development credentials or Google pilot sessions. The desktop account can connect to it. Time entries remain local.
Phase 3 starts with a separate [durable operation queue](docs/sync-client.md).
The queue can create, edit, and delete timed and duration entries with optional projects through the local development API.
It can retrieve entry pages, assigned projects, and ordered remote changes, including deletions.
It checks account identity and responses, preserves retry keys, and retains conflicts for explicit resolution.
The optional [sync worker](docs/sync-recovery.md) schedules retries and supports reviewed conflict choices.
The API and queue preserve captured rounding settings for offline entries.
The desktop app still uses its existing local database.
An isolated [Google sign-in check](docs/google-signin.md) can validate the pilot registration. The separate API session check is described in [API sessions](docs/api-sessions.md).
The API now has [personal settings](docs/settings.md) for rounding, the weekly target, and the visual indicator.
Defaults are five-minute rounding, 35 hours per week, and the progress indicator. The native app has separate local settings with the same defaults.
The API also has [saved time zones and project access checks](docs/timed-projects.md), with private, paginated project lists.
[Project administration and remote retrieval](docs/retrieval-admin.md) add explicit admin grants, versioned membership updates, and durable change checkpoints.

## Current scope

- A widget that runs up to three tasks at the same time, each with a play and pause button.
- A Timer screen with running tasks, a Calendar view, Day blocks, and a List view.
- The Calendar view shows one week on an hour grid, with day totals, a week total, and project shares.
- The List view groups the sessions since the start of last week by day.
- A Projects screen to create, edit, archive, and restore projects with a colour and an optional client.
- Completed timed entries and duration-only entries, with editing and deletion.
- Local rounding, weekly targets, and a weekly progress bar.
- A Settings screen with an optional, one-time import from Toggl.

See [local entries and settings](docs/local-refinement.md) for the entry rules and keyboard controls.
The entry form expands to fit its fields and action buttons without an internal scroll area.

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

For the separate Xero demo connection check, see [the test tool instructions](docs/xero-demo.md).
This development tool does not add Xero integration to the desktop app.
Its optional `--write-test` mode checks labelled demo time entries and removes them after the test.

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
- Connect the native app after the remaining API and queue prerequisites pass their checks.

## Fonts

Trackfast bundles Inter and JetBrains Mono from `assets/fonts/`.
Both fonts use the SIL Open Font License 1.1. The license files are next to the fonts.

## Documentation

- [Architecture](docs/architecture.md)
- [Interface design](docs/interface.md)
- [Toggl import rules](docs/toggl-import.md)
- [Contribution guide and checks](CONTRIBUTING.md)

## Google pilot sessions

The API now supports Google sign-in with stored membership and expiring Trackfast sessions.
Renewal, sign-out, and member disabling are available in a separate local mode.
See [API sessions](docs/api-sessions.md) for setup, endpoint rules, tests, and remaining limits.
The native app remains on local storage. Shared hosting is not enabled.

The separate queue client now has [secure session handling](docs/client-sessions.md).
It stores refresh credentials in the operating system credential store and supports renewal, restart recovery, and sign-out cleanup.
The native app is not connected to this client yet.
