# Trackfast

Trackfast is a native Rust and egui scaffold for a Toggl Track desktop client.
It follows Spotifast's architecture and uses the supplied Toggl screenshots as a visual reference.
The intended GitHub repository is BilliGraham/trackfast. Keep it private.

## Current scope

- A main window with Timer and Projects navigation.
- An empty timer list with daily and weekly totals.
- A separate widget that opens from the main window.
- Three placeholder task rows in the widget.
- An always-on-top request for the widget.

Timer controls and project creation are disabled placeholders.
There is no authentication, network access, timer logic, storage, or Toggl sync.
No account details or task content from the supplied screenshots are included.
The interface uses dark surfaces and pink accents. It is not a finished design.

## Run

Install Rust 1.98.0 and the native build tools for your platform.
Run these commands from the repository root:

    cargo generate-lockfile
    cargo run --locked

The first build creates Cargo.lock. Commit that file after dependency resolution.
The scaffold environment could not reach crates.io, so no lockfile is included yet.

## Checks

    cargo fmt --all --check
    cargo clippy --locked --all-targets -- -D warnings
    cargo test --locked --all-targets
    cargo build --locked

## Planned workflow

The main window will contain timer entry, project selection, and a list grouped by day.
Projects will supply names and colours for entries and widget tasks.
The widget will open from the main window, like Spotifast's mini player.
It will hold up to three tasks with independent start and stop controls.
Multiple timers may run at the same time.

See [the architecture](docs/architecture.md),
[the integration boundaries](docs/toggl-boundaries.md),
and [the contribution guide](CONTRIBUTING.md).
