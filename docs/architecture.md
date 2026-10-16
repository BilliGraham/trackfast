# Architecture

| Module | Responsibility |
| --- | --- |
| src/main.rs | Start the native window and select the initial theme. |
| src/model.rs | Hold navigation state, widget visibility, and action types. |
| src/app.rs | Apply actions and manage the widget viewport. |
| src/ui/mod.rs | Draw the main window, empty screens, and widget slots. |

Views emit actions. The application applies them after drawing.
The widget is a second native viewport in the same process.
It opens from the main window, not from the existing Toggl website.
It requests the operating system's always-on-top window level.
Window managers can restrict that behaviour.
The scaffold has no continuous repaint loop.

## Reference decisions

Spotifast separates views, application actions, and background services.
Trackfast follows that structure. It uses published eframe with the glow renderer.
It does not need Spotifast's audio engine or its audio-related patches.
No fastframe component is required for the current shell.
Assess individual shared components when a concrete need exists.

## Future boundaries

A later model will own independent intervals for up to three widget tasks.
Both windows will read the same model.
Background services will own disk writes and Toggl requests.
Credentials will use the operating system's protected credential store.
A durable outbox will preserve completed intervals until sync succeeds.
Do not retry an uncertain create request before checking for duplicates.
These are design boundaries. This scaffold does not implement those services.
