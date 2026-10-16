# Architecture

| Module | Responsibility |
| --- | --- |
| `src/main.rs` | Start the native window and select the initial theme. |
| `src/model.rs` | Hold navigation state, widget visibility, and action types. |
| `src/lib.rs` | Export the standalone timer module. |
| `src/timer.rs` | Hold task descriptions, timer state, and seven unit tests. |
| `src/app.rs` | Apply actions and manage the widget viewport. |
| `src/ui/mod.rs` | Draw the main window, empty screens, and widget slots. |

## Interface flow

1. Views draw the interface and return actions.
2. The application applies the actions after the views return.
3. No view changes application state while it draws.

The widget is a second native viewport in the same process.
It opens from the main window, not from the Toggl website.
It requests the always-on-top window level from the operating system.
Some window managers ignore or restrict that request.
The scaffold has no continuous repaint loop.

## Dependencies

Trackfast uses eframe from crates.io with the glow renderer.
Default eframe features are off. `Cargo.toml` lists each enabled feature.

## Timer model

Each `Task` owns its description and `TimerState`.
`TimerState` is either stopped or running.
The model uses `std::time::Duration` and accepts timestamps from a common monotonic origin.
It does not sample a clock or access the interface.
It stores accumulated time across active intervals and excludes pauses.
It does not store individual completed intervals for later sync.
The interface does not use this model yet.

## Future boundaries

These are design boundaries. The scaffold does not implement these services.

- The interface will connect the model to up to three widget tasks.
- The main window and the widget will read the same model.
- Background services will own disk writes and Toggl requests.
- Credentials will use the protected credential store of the operating system.
- A durable outbox will keep completed intervals until sync succeeds.
- Before the app retries an uncertain create request, it must check for a duplicate entry.
