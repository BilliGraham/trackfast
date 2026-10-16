# Trackfast agent guide

Read `CONTRIBUTING.md` before you change this repository.
Use ASD-STE100 sentence structure for prose. Keep necessary technical terms.

## Scope

Trackfast is a local time tracker. Its design is inspired by Toggl Track, but it is not a Toggl client.
Add features in small steps that the user requests.
All data stays in the local SQLite file. Do not add sync, accounts, or Toggl writes without a new task.
The Toggl import is read-only and runs only when the user starts it. Use only the official Toggl Track API.
Do not use Toggl branding in the interface.
Keep the app native. Do not add a browser engine, telemetry, or a hosted backend.

## Platforms

Prioritize Linux and Windows for implementation and validation.
Keep the code portable to macOS. macOS has lower priority for now.
Do not report Linux or macOS as tested until those checks run.

## Architecture

- `src/ui/` draws views and returns `Action` values.
- `src/app.rs` applies each `Action` after the view returns.
- `src/model.rs` holds shared interface state and action types.
- `src/entries/` holds the session types and the day and week logic that the views use.
- `src/db/` owns the SQLite schema. Only the database worker thread opens the file.
- Make each user action one transaction. Never leave a session or widget row half changed.
- Change the schema only by adding a step to `MIGRATIONS`. Never edit a released step.
- `src/import.rs` and `src/toggl/` run the optional Toggl import on a background thread.
- Never store the import token. Never log credentials or authorization responses.
- Network and disk work must run outside the interface thread.
- `src/lib.rs` and `src/timer.rs` are a standalone timer model. The app does not use them.
- Keep platform code behind target-specific modules or `cfg` blocks.
- Prefer crates.io dependencies. Pin any future Git dependency to a reviewed commit.
- Add a dependency only when the current task needs it.

The main window and the widget read the same `State`.
Closing the widget must not close the main window.
Read `docs/architecture.md` before you change the database. Read `docs/toggl-import.md` before you change the import.

## Comments

Write comments only for actionable TODO or FIXME items, hidden business rules,
or the reason for a non-obvious decision. Do not restate the code.

## Validation

Run the checks in `CONTRIBUTING.md`.
Add focused tests when behaviour can regress. Do not test declarations alone.
Keep review captures and recordings outside the repository.
Report compiled, tested, and visually checked platforms separately.
Update `README.md` and the relevant files in `docs/` with user-visible changes.

## Git

Keep `main` linear. Use focused commits. Do not create merge commits.
Do not rewrite published history without explicit user permission.
Keep the GitHub repository private under BilliGraham.
Do not publish releases or add release workflows without a user request.
