# Architecture

| Module | Responsibility |
| --- | --- |
| `src/main.rs` | Start the native window and select the initial theme. |
| `src/model/widget.rs` | Own up to three selected entries and their last known data. |
| `src/model.rs` | Hold navigation state, Toggl connection state, and action types. |
| `src/lib.rs` | Export the standalone timer module. |
| `src/timer.rs` | Hold task descriptions, timer state, and seven unit tests. |
| `src/app.rs` | Apply actions, receive sync events, and manage the widget viewport. |
| `src/ui/` | Draw the main window, the sign-in form, entries, projects, and widget slots. |
| `src/sync.rs` | Run Toggl requests and credential-store calls on a background thread. |
| `src/toggl/client.rs` | Send blocking Toggl Track API v9 requests and map their errors. |
| `src/toggl/quota.rs` | Count requests and calculate sync deadlines with a monotonic clock. |
| `src/toggl/credentials.rs` | Read, write, and delete the API token in the credential store. |
| `src/toggl/summary.rs` | Group entries by local day and calculate totals. |
| `src/toggl/week.rs` | Split entries at local midnight, place overlapping entries in lanes, and total projects. |
| `src/ui/calendar.rs` | Draw the week navigator, project shares, day headers, and the hour grid. |

## Interface flow

1. Views draw the interface and return actions.
2. The application applies the actions after the views return.
3. No view changes application state while it draws.

The sign-in text field is the one exception. egui needs a mutable buffer for text input,
so the application gives the view a draft string. The application takes and clears the draft on sign-in.
The field clears its undo history after input. Submission and sign-out also remove its stored edit state.

The widget is a second native viewport in the same process.
Both windows read the same `State`. Widget actions return to `App` after the widget view returns.
Toggl entry IDs identify selections. The model rejects duplicate selections and a fourth row.
Refreshes update selected entries without replacing their IDs or changing their order.
Missing entries keep their last known data. A missing running entry stops accumulating time until it appears in a snapshot again.
Sign-out clears the selected data and closes the widget.
Selections stay in memory only. The standalone timer model remains separate.
It opens from the main window, not from the Toggl website.
It requests the always-on-top window level from the operating system.
Some window managers ignore or restrict that request.
The app repaints after input and after each sync event.
While a Toggl entry runs, the app also repaints once per second to keep the timer current.

## Sync flow

1. At start, the worker reads the token from the credential store.
2. If a token exists, the worker loads a snapshot. If not, the interface shows the sign-in form.
3. On sign-in, the worker loads a snapshot first. It saves the token only after Toggl accepts it.
4. The worker sends `Busy`, `RequestLimit`, `Loaded`, `Failed`, or `SignedOut` events through a channel.
5. The application reads all waiting events at the start of each frame.

A network failure keeps the last snapshot on screen and shows the error.
A rejected token removes the active client and saved token, then returns the interface to sign-in.
The worker preserves typed API errors until it selects the account state.
Tests use a fake backend for API and credential operations.

The worker owns one rolling request budget for startup, sign-in, and refresh.
The interface reads the worker's next permitted attempt time. It does not enforce the budget itself.
Each HTTP request consumes budget before transmission. Quota errors block all attempts for one hour.
The token never goes into the model, a log, or an error message.

## Dependencies

| Crate | Use |
| --- | --- |
| eframe | Native window and interface with the glow renderer. Default features are off. |
| ureq | Blocking HTTPS client with rustls. One background thread makes an async runtime unnecessary. |
| serde | Decode Toggl JSON. |
| jiff | Time zones, local dates, and RFC 3339 times. |
| keyring | Platform credential stores. |
| base64 | Encode the basic authentication header. |

The timer model in `src/lib.rs` uses only the standard library.

## Timer model

Each `Task` owns its description and `TimerState`.
`TimerState` is either stopped or running.
The model uses `std::time::Duration` and accepts timestamps from a common monotonic origin.
It does not sample a clock or access the interface.
It stores accumulated time across active intervals and excludes pauses.
It does not store individual completed intervals for later sync.
The interface does not use this model yet.

## Future boundaries

These are design boundaries. The app does not implement these services yet.

- The interface will connect the model to up to three widget tasks.
- The main window and the widget will read the same model.
- Background services will own disk writes and Toggl writes.
- A durable outbox will keep completed intervals until sync succeeds.
- Before the app retries an uncertain create request, it must check for a duplicate entry.
