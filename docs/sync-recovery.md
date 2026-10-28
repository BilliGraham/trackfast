# Scheduled sync and conflict recovery

The separate Rust sync client provides an optional worker and explicit conflict choices.
The native app uses its local database and an explicit [delivery controller](native-delivery.md). Xero records are unchanged.

## Worker lifecycle

Run `scheduler::run` on a dedicated thread. Supply a queue, a `SessionClient`, a command receiver, and an event sender.
Use one worker per account queue. Do not open a second writer for that queue while the worker runs.
The session client retains its account lock until the worker stops.

Each cycle sends at most one pending operation and retrieves at most one change page.
The worker sends pending operations before it reads changes. This order preserves replay recovery after a lost successful response.
More pending work or pages schedule another cycle after 100 milliseconds. An idle successful cycle schedules another check after 30 seconds.
`DevelopmentClient::sync_batch` and `SessionClient::sync_batch` also expose bounded delivery for other callers.
The existing `sync` method still sends all available pending operations until a failure occurs.

Transient transport errors and server failures use exponential retry delays, starting at five seconds.
Each delay includes up to 20 percent random variation. The total delay stops increasing at five minutes.
HTTP 429 uses the longer of this delay and `Retry-After`. Both integer seconds and HTTP dates are supported.
The queue stores the deadline before the worker waits. Restarting the client preserves the deadline and failure count.
An explicit `Resume` command cannot bypass a stored server retry deadline.

Authentication failures, conflicts, rejected requests, invalid responses, and configuration errors pause the worker.
Credential storage errors also pause it. A queue storage failure returns an error and stops the worker.
An ambiguous session renewal requires sign-out recovery and a new sign-in. The worker does not retry the old refresh credential.

Read `Queue::sync_schedule` before transferring the queue to the worker to obtain its saved state.
`WorkerEvent::Schedule` reports the result of each attempted cycle. It contains no credentials or entry text.
Send `Stop`, or drop the command sender, to stop between cycles. Requests already in progress finish under transport timeouts.
Wait for the worker thread to finish before opening the queue for conflict resolution or another session client.
After correction, call `Queue::resume_sync` before restarting, or send `Resume` to an existing worker when appropriate.
These APIs do not provide an interface dialog or a native sign-in screen.

## Conflict choices

`Queue::conflicts` returns local pending work and any stored remote snapshot.
A missing snapshot requires another change-feed read before the remote and copy-as-new choices are available.
The worker retrieves remaining change pages before it pauses for conflicts. A conflict pauses delivery for the entire queue.

The caller must obtain an explicit user choice:

- `resolve_with_remote` accepts the reviewed remote entry or deletion. It removes the local pending operation.
- `resolve_with_local` sends the local values against the reviewed remote version with a new operation key.
- `resolve_as_new` preserves the local content under a new entry ID and accepts the remote original. It creates a new operation key.

The remote and copy-as-new choices require the exact stored version that the caller reviewed.
The local choice checks a stored snapshot when available. Existing callers can still supply a version from a direct remote read.
A creation collision requires a stored live snapshot before the local choice can replace the remote entry.
A known remote deletion requires acceptance or a copy as a new entry. A pending deletion cannot be copied.
Another server change can cause a new conflict. No choice bypasses the server version check.

Each choice uses one SQLite transaction. A failed write preserves the local entry, pending operation, and remote snapshot.
Remote deletions retain their entry IDs to prevent reuse. Older feed pages cannot replace a newer conflict snapshot.

## Validation

New tests failed before implementation for conflict choices, scheduling, bounded delivery, and retry header handling.
The checks include rollback, stale versions, deletion recovery, restart deadlines, explicit pauses, and worker shutdown during a retry wait.
The HTTP integration test uses a disposable Postgres database and generated SQLite files.
It checks bounded delivery, acceptance of remote edits, and preservation of local work after a remote deletion.

On October 27, 2026, formatting, Clippy, tests, and builds passed for the sync client and API on Windows and Linux.
The client passed 45 library tests and one example test. Its optional native credential-store test did not run in this step.
The API passed 34 tests. Linux checks used a disposable Rust 1.98 container.
The native app passed all 67 tests, formatting, Clippy, and build checks on Windows.
All 14 API contract checks passed on Windows. macOS and visual checks did not run. The interface did not change.
