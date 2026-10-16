# Trackfast agent guide

Read `CONTRIBUTING.md` before you change this repository.
Use ASD-STE100 sentence structure for prose. Keep necessary technical terms.

## Scope

The app has a scaffold and a standalone timer model.
Add features in small steps that the user requests.
The current step adds the timer model and seven tests.
Do not connect the model to the interface, authentication, persistence, or sync without a new task.
Keep the app native. Do not add a browser engine, telemetry, or a hosted backend.
Use official Toggl APIs when integration work starts.

## Platforms

Prioritize Linux and Windows for implementation and validation.
Keep the code portable to macOS. macOS has lower priority for now.
Do not report Linux or macOS as tested until those checks run.

## Architecture

- `src/ui/` draws views and returns `Action` values.
- `src/app.rs` applies each `Action` after the view returns.
- `src/model.rs` holds shared interface state and action types.
- `src/lib.rs` exports the standalone timer model.
- `src/timer.rs` owns task descriptions and independent timer states.
- Timer tests supply timestamps from a common monotonic origin. Do not read a clock inside the model.
- Future network and disk work must run outside the interface thread.
- Future state writes must be atomic and backward compatible.
- Never log credentials or authorization responses.
- Keep platform code behind target-specific modules or `cfg` blocks.
- Prefer crates.io dependencies. Pin any future Git dependency to a reviewed commit.
- Add a dependency only when the current task needs it.

The main window and the widget must share one model when timers exist.
Closing the widget must not close the main window.
Do not implement parallel tasks as repeated server timer starts.
Read `docs/toggl-boundaries.md` before you change integration behaviour.

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
Do not publish releases or add release workflows during scaffold work.
