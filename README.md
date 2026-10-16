# Trackfast

Trackfast is a native Rust and egui desktop client for Toggl Track.
The supplied Toggl screenshots are its visual reference.
The GitHub repository is BilliGraham/trackfast. Keep it private.

## Current scope

- Sign-in with a Toggl Track API token, kept in the credential store of the operating system.
- A read-only Timer screen with the running entry, a Calendar view, and a List view.
- The Calendar view shows one week on an hour grid, with day totals, a week total, and project shares.
- Use the arrows to show earlier weeks. Each earlier week that is not loaded uses one Toggl request.
- The List view groups the entries since the start of last week by day, with daily and weekly totals.
- A read-only Projects screen, grouped by workspace.
- A manual Refresh button.
- A separate widget that opens and closes from the main window, with up to three pinned Toggl entries.
- An always-on-top request for the widget.

Trackfast does not change any data in Toggl.
Timer controls and project creation are disabled placeholders.
The standalone timer model supports start, stop, resume, and overlapping tasks.
The interface does not use the model yet.
Trackfast keeps time data and widget selections in memory. It loads the data again after each start.
The repository contains no account details or task content from the screenshots.
The interface uses dark surfaces and pink accents. The design is not final.

## Widget

Open widget shows the running entry on first use. If no entry runs, it shows the most recent entry.
If there are no entries, the widget shows an empty row.

Move the pointer over the widget. Open the ⋮ menu at the lower left and select Add task.
The widget holds a maximum of three entries. Add task is disabled when the widget is full.
The menu also has a Close widget action.
In the Calendar view, hover over an entry to show its menu with three vertical dots.
The List view also has this menu for each entry.
Select Add to widget.
This also opens the widget.
The action is disabled when the entry is already selected or the widget is full.
Use the remove button on a row to remove that selection. This does not stop or change the Toggl entry.

Closing the widget and refreshing Toggl preserve the selected rows.
Sign-out or app exit clears the selections. The widget does not save them to disk.
If a selected entry leaves the loaded history, its row shows the last known time and a history notice.

## Run

Install [rustup](https://rustup.rs) and the native build tools for your platform.
On Linux, also install the libxkbcommon, Wayland, and Mesa OpenGL development packages.
`rust-toolchain.toml` selects Rust 1.98.0. Rustup installs it on the first build.

Run this command from the repository root:

```bash
cargo run --locked
```

## Connect to Toggl

1. Open your [Toggl Track profile](https://track.toggl.com/profile).
2. Copy the API token.
3. Paste the token into Trackfast and select Connect.

Trackfast keeps the token in Windows Credential Manager, the macOS Keychain, or the Linux Secret Service.
On Linux, a Secret Service provider such as GNOME Keyring or KWallet must run.
Select Sign out to remove the saved token and clear the token field's edit history.
A rejected saved token returns you to sign-in.

Free Toggl plans allow 30 requests per hour to the endpoints that Trackfast uses.
Each sync uses two requests. Trackfast does not poll.
It syncs once at start, after you connect, and when you select Refresh.
All sync attempts share a limit of 30 request attempts in a rolling hour.
Trackfast waits at least four minutes between sync attempts, including startup and sign-in.
After HTTP 402 or 429, it blocks further attempts for one hour. It does not retry automatically.
These limits apply to the current process. Other apps and restarts can still consume the shared Toggl quota.

## Platforms

Linux and Windows are the main development and validation targets.
Keep the design portable to macOS. macOS has lower priority for now.
The timer model uses only the Rust standard library.
A separate CI job compiles and runs its tests on Linux and Windows.

## Timer model

The first TDD step is complete in `src/timer.rs`.
Seven tests cover initial state, elapsed time, stop, resume, overlapping tasks, repeated starts, and repeated stops.
Tests supply timestamps. They do not read a clock or wait for time to pass.

Run the timer tests after dependency resolution:

```bash
cargo test --locked --lib
```

See [the timer guide](docs/timer.md) for the model and the TDD sequence.

## Planned workflow

- The main window will contain timer entry, project selection, and a list grouped by day.
- Projects will supply names and colours for entries and widget tasks.
- The widget will open from the main window.
- The widget will hold up to three tasks with independent start and stop controls.
- More than one timer can run at the same time.

## Fonts

Trackfast bundles Inter and JetBrains Mono from `assets/fonts/`.
Both fonts use the SIL Open Font License 1.1. The license files are next to the fonts.

## Documentation

- [Architecture](docs/architecture.md)
- [Interface design](docs/interface.md)
- [Toggl integration boundaries](docs/toggl-boundaries.md)
- [Contribution guide and checks](CONTRIBUTING.md)
