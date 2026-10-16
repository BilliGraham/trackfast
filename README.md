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
- A separate widget that opens from the main window, with three placeholder task rows.
- An always-on-top request for the widget.

Trackfast does not change any data in Toggl.
Timer controls, project creation, and the widget tasks are disabled placeholders.
The standalone timer model supports start, stop, resume, and overlapping tasks.
The interface does not use the model yet.
Trackfast has no local data cache. It loads the data again after each start.
The repository contains no account details or task content from the screenshots.
The interface uses dark surfaces and pink accents. The design is not final.

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

## Documentation

- [Architecture](docs/architecture.md)
- [Interface design](docs/interface.md)
- [Toggl integration boundaries](docs/toggl-boundaries.md)
- [Contribution guide and checks](CONTRIBUTING.md)
