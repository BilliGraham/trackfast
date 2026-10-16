# Trackfast agent guide

Read CONTRIBUTING.md before changing this repository.
Use ASD-STE100 sentence structure for prose. Keep necessary technical terms.

## Scope

This repository is a scaffold. Add features in small steps requested by the user.
Do not implement timer logic, authentication, persistence, or sync during scaffold work.
Keep the app native. Do not add a browser engine, telemetry, or a hosted backend.
Use official Toggl APIs when integration work starts.

## Architecture

- src/ui/ draws views and returns Actions.
- src/app.rs applies Actions after the view returns.
- src/model.rs holds shared interface state and action types.
- Future network and disk work must run outside the interface thread.
- Future state writes must be atomic and backward compatible.
- Never log credentials or authorization responses.
- Keep platform code behind target-specific modules or cfg blocks.
- Prefer crates.io dependencies. Pin any future Git dependency to a reviewed commit.
- Add a dependency only when the current task needs it.

The main window and widget must share one model when timers are added.
Closing the widget must not close the main window.
Do not implement parallel tasks as repeated server timer starts.
Read docs/toggl-boundaries.md before changing integration behaviour.

## Comments

Write comments only for actionable TODO or FIXME items, hidden business rules,
or the reason for a non-obvious decision. Do not restate the code.

## Validation

Run the checks in CONTRIBUTING.md.
Add focused tests when behaviour can regress. Do not test declarations alone.
Keep review captures and recordings outside the repository.
Report compiled, tested, and visually checked platforms separately.
Update the README and relevant reference files with user-visible changes.

## Git

Keep main linear. Use focused commits. Do not create merge commits.
Do not rewrite published history without explicit user permission.
Keep the GitHub repository private under BilliGraham.
Do not publish releases or add release workflows during scaffold work.
