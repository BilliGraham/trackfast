# Contributing to Trackfast

This guide adapts Spotifast's contribution principles to a time-tracking app.

## Product scope

Build one small step at a time. The current step is the scaffold.
The planned product has a timer screen, a list grouped by day, project setup,
and a separate widget with three independent tasks.
Do not add reports, calendars, billing, or team administration without a new request.

## Design principles

1. Keep startup work, idle work, memory use, and dependencies small.
2. Keep network and disk waits outside the interface thread.
3. Update controls immediately when later timer actions become available.
4. Preserve local work when a request fails.
5. Show actionable failures without exposing private data.
6. Use official APIs and respect their limits.
7. Keep platform-specific code isolated.

Use the supplied screenshots as a visual reference.
Use dark surfaces, narrow navigation, pink selection accents, and project colours.
Do not copy branding, account details, or task data.
Keep the widget separate from the main window.

## Changes

Keep each change focused. State the problem, changed behaviour, and validation.
Discuss larger changes before implementation. Prefer useful defaults over new settings.
Use crates.io dependencies unless a reviewed fix requires a pinned Git source.
Do not carry Spotifast's audio, skin, or packaging dependencies into this app.

## Checks

If Cargo.lock is absent, run cargo generate-lockfile first and commit the result.

    cargo fmt --all --check
    cargo clippy --locked --all-targets -- -D warnings
    cargo test --locked --all-targets
    cargo build --locked

Add focused regression tests when timer and sync behaviour exists.
Inspect interface changes at narrow and normal window sizes.
Check both themes when both themes become supported.
Keep review captures and recordings outside the repository.
Report platform coverage honestly. CI configuration alone is not evidence of a passing build.

## Attribution

Spotifast supplied the architecture and contribution-policy reference.
See THIRD_PARTY_NOTICES.md for the source and its license.
The app source is new. It contains no Spotify implementation code.
