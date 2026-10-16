# Trackfast

Trackfast is a native Rust and egui scaffold for a Toggl Track desktop client.
The supplied Toggl screenshots are its visual reference.
The GitHub repository is BilliGraham/trackfast. Keep it private.

## Current scope

- A main window with Timer and Projects navigation.
- An empty timer list with daily and weekly totals.
- A separate widget that opens from the main window.
- Three placeholder task rows in the widget.
- An always-on-top request for the widget.

Timer controls and project creation are disabled placeholders.
The standalone timer model supports start, stop, resume, and overlapping tasks.
The interface does not use the model yet.
There is no authentication, network access, storage, or Toggl sync.
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
- [Toggl integration boundaries](docs/toggl-boundaries.md)
- [Contribution guide and checks](CONTRIBUTING.md)
