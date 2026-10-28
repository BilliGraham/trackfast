# Native entry delivery

The native app can send completed timed and duration entries to the local Trackfast API.
It uses the saved account binding, creation boundary, and explicit project mappings.
Existing history and imported entries remain local. This step does not export to Xero.

## Start and stop

1. Start the local API and its database.
2. Sign in through the configured native app.
3. Complete [sync setup](native-sync-setup.md) and the required project mappings.
4. Select **Start delivery** in Settings.

Delivery continues while the app remains open. Select **Stop delivery** to stop subsequent attempts.
A request already in progress can finish. Signing out also stops delivery.
Start delivery again after restarting the app. Pending work and retry deadlines survive restart.
Work with an unmapped project waits locally. Unassigned work uses the API's unassigned option.
Running timers wait until stopped. Entries that were already running when setup was saved remain local.

## Durable transfer

### Entry status

The list, Day blocks, and calendar duration rows show each entry's API sync status.
Timed calendar blocks show it in the hover details. Hover over a label for an explanation.

| Label | Meaning |
| --- | --- |
| Local only | Sync is not configured, or the entry is old history or an import. |
| Waiting for API | The current revision has no receipt. It remains local while delivery is stopped or offline. |
| Sync after stop | The new entry has a running timer. |
| Map project to sync | The entry needs an explicit project mapping in Settings. |
| API confirmed | The API confirmed the current revision. This does not indicate a Xero export. |
| Sync needs attention | Delivery stopped for this pending entry after a failure. Check Settings before retrying. |

A later edit changes a confirmed entry back to waiting. Confirmation of an older revision does not confirm that edit.
Migration 6 retains the attention flag across restart. Successful confirmation removes the flag with its outbox operation.
Deleted entries have no entry card. Delivery failures for deletions remain visible in Settings during that run.

### Stored operations

Native migration 5 adds a single frozen outbox and a confirmed revision per entry.
The existing entry identity triggers record local changes in the same transaction as each user action.
The database worker later reads the current eligible revision and saves one immutable operation in a transaction.
An operation contains a stable entry ID, operation ID, payload, and captured rounding settings.
The operation retains its payload through retries, even if the user edits or deletes the native entry.
After confirmation, a later revision creates a separate edit or deletion operation.
Changes made before preparation can be combined into the latest local state. An entry deleted before preparation is never uploaded.

The account worker owns `native-sync-queue.db` beside the native database. It never opens the native database itself.
Queue migration 5 records each transfer fingerprint and confirmation state.
Staging the transfer, entry, and pending operation uses one queue transaction.
Replaying the same transfer returns its existing state. Reusing its operation ID with different content fails.
The queue records confirmation only in the same transaction that accepts the verified API receipt.
The native worker then saves the confirmed revision and removes its outbox in one transaction.
If either process step fails, the same operation remains available for replay after restart.
The fingerprint ledger avoids retaining another copy of completed entry text.

Timed payloads preserve exact instants and use the saved time zone's offsets, including different offsets across a clock change.
Captured rounding Off sends exact recorded seconds. Five-minute rounding sends the recorded snapshot so the API derives the effective duration.
Changing current settings does not change a frozen operation or an existing entry's recorded rule.

## Retry and conflict limits

Delivery sends at most one operation per cycle. Network work remains outside the interface and native database threads.
Transport failures use the queue's persistent retry schedule. Server retry deadlines also apply after an explicit restart of delivery.
Authentication failures, invalid responses, rejected requests, storage failures, and conflicts stop delivery.
The status message reports the pause. Local entries and pending operations remain available.

Normal delivery sends changes only. Reviewed conflicts can apply one accepted API snapshot. General remote edits are not downloaded into native entries.
Edits and deletions use the version from the last confirmed API receipt. A newer remote version causes a conflict.
No automatic overwrite or merge occurs. Settings now provides [native conflict review](native-conflicts.md) for explicit choices.
Do not repeatedly restart a conflicted or rejected operation without resolving its cause.

## Validation

The first tests failed before transfer staging, native outbox methods, push-only scheduling, and delivery controls existed.
A timed-entry test then found mismatched UTC text and local-zone metadata. The corrected payload retains each endpoint's local offset.
Tests cover stable keys, immutable replay, edits during delivery, deletions, project mapping, captured rounding, account scope, and transaction rollback.
A two-file restart test simulates an API receipt saved in the queue but not yet acknowledged in the native database.
The API HTTP recovery fixture now uses native transfers for lost create, edit, and delete responses.
It checks one API entry record and exactly three operation receipts and audit events.

On October 28, 2026, all 99 native tests passed on Windows and Linux, including the seven timer tests.
The queue passed 51 library tests and one example test on Windows and Linux. Windows also passed with the native dependency versions.
The optional operating-system credential-store test did not run. Formatting, Clippy, and builds passed on Windows.
All 35 API tests passed on Windows and Linux against a disposable Postgres database, including the HTTP recovery fixture. Native, queue, and API builds passed on both platforms.
The visual helper failed during initialization and after reset. Native visual checks and macOS checks did not run.
No live entry delivery or pilot data changes were performed during these checks.

The later live Windows check used an unassigned 13-minute entry with five-minute rounding enabled.
A read-only database query confirmed 780 original seconds, 900 effective seconds, version 1, one audit event, and one saved receipt.

The entry-status test failed before the snapshot API existed. The visible-label test failed before the interface displayed status.
The status step passed all 102 native tests on Windows and Linux. Windows formatting and Clippy passed. Layout checks cover both themes and narrow labels.
The running Windows app locked the standard executable. A separate executable is used for this build check.
Windows and Linux builds passed. Live visual checks and macOS checks remain unperformed.
