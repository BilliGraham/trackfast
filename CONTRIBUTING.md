# Contributing to Trackfast

## Product scope

Build one small step at a time.
The current steps are the standalone timer model and read-only Toggl sync.
The planned product has these parts:

- A timer screen.
- A list of entries grouped by day.
- Project setup.
- A separate widget with three independent tasks.

Do not add reports, calendars, billing, or team administration without a new request.

## Design principles

1. Keep startup work, idle work, memory use, and dependencies small.
2. Keep network and disk waits outside the interface thread.
3. Show the result of a timer action immediately. Do not wait for the server.
4. Keep local work when a request fails.
5. Show failures that the user can act on. Do not show private data in them.
6. Use official APIs and obey their limits.
7. Keep platform-specific code isolated.

Use the supplied screenshots as a visual reference.
Use dark surfaces, narrow navigation, pink selection accents, and project colours.
Do not copy branding, account details, or task data.
Keep the widget separate from the main window.

## Changes

Keep each change focused. State the problem, the changed behaviour, and the validation.
Discuss larger changes before you start them. Prefer good defaults to new settings.
Use crates.io dependencies. Use a Git source only for a reviewed fix, and pin it to a commit.
Add a dependency only when the current change needs it.

## Checks

Run these commands from the repository root:

```bash
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo build --locked
```

Commit `Cargo.lock` when dependencies change.
Use TDD for timer changes. Run one new test before changing the implementation.
If it already passes, record that result. Do not introduce a defect to force a failure.
Run the full timer suite after each change.
Add focused regression tests for sync and date behaviour.
Do not call the real Toggl API from tests.
Examine interface changes at narrow and normal window sizes.
Examine both themes when the app supports both themes.
Keep review captures and recordings outside the repository.
Report each platform as compiled, tested, or visually checked.
A CI configuration alone does not prove that a build passes.
