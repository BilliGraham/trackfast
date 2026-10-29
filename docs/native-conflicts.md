# Native conflict review

Settings provides conflict review for native entries whose delivery paused after an API version conflict.
[Remote-change sync](native-remote-sync.md) also sends competing edits to this review. Xero export remains separate work.

## Review and choose

1. Open Settings with the original account connected.
2. Select **Review conflicts**. This stops delivery and reads one bounded API change page.
3. Select **Load more API changes** while more pages remain.
4. Compare the submitted local values with the stored API snapshot.
5. Select one choice. The choice resumes delivery.

| Choice | Result |
| --- | --- |
| Use local version | Retry the submitted values against the reviewed API version with a new request key. Later local edits remain queued. |
| Use API version | Accept the reviewed API entry or deletion. Preserve later local edits as a separate new entry. |
| Keep a local copy | Accept the API version and preserve current local work under a new entry ID. The copy is eligible for delivery. |

Later local deletions remain queued when the reviewed API entry still exists.
If both versions are deleted, no extra deletion is needed.
A pending deletion cannot be copied. A remote deletion cannot be replaced through the local choice.
The local choice also requires matching recorded rounding rules.

The panel shows the frozen submission, not an editable draft. A later local edit can therefore differ from the displayed submission.
No automatic merge occurs. A newer API version can cause another conflict after a local retry.
Accepting the API version accepts the snapshot shown. It does not claim that no further API change exists.

## Recovery

Queue migration 6 saves each resolution and any replacement operation key in one transaction.
The immutable native transfer retains its original key. A local retry confirms that original transfer only after a verified API receipt.
API acceptance records a reviewed result instead. The native worker must apply this result before acknowledging the transfer.
The account worker distinguishes a resolution from an ordinary delivery receipt.

Native migration 7 records applied resolution keys. Applying the result, preserving a copy, updating revision state, and recording completion use one transaction.
A restart between queue completion and native application replays the saved result. A repeated application does not create another copy.
Failed native writes retain the outbox and leave the reviewed result available for retry.
The queue retains resolution snapshots for recovery. They contain entry data, not credentials.

Conflict reads do not update unrelated queue entries or the normal remote-change checkpoint.
Explicit review reads separate pages from normal delivery. Each click reads at most 100 changes.
Choices stay disabled while pages remain. Failed reads keep pending work and require another explicit review attempt.
Signing out clears the visible review. Stored pending work remains bound to the original account.

Applying an API project requires an explicit local mapping. The current local project is preferred when it maps to that API project.
Otherwise, exactly one local mapping must exist. Missing or ambiguous mappings stop native application and preserve the saved result.
Mapping reassignment and ambiguous reverse-mapping selection remain later work. No project is guessed from its name.

## Validation

Tests failed before queue resolution methods, native application, and the review panel existed.
A stale-baseline regression test also failed before its version guard was added.
Tests cover retry receipts, repeated conflicts, remote deletion, transaction rollback, later local edits and deletions, and replay across two database files.
The panel test compares both versions and checks the selected action at 340 and 600 pixels in both themes.
The HTTP test uses a disposable Postgres database and generated queue files.

On October 28, 2026, Windows and Linux passed 107 native tests, 55 queue library tests, one queue example test, and 36 API tests.
The optional operating-system credential test did not run. All builds passed. Windows formatting and Clippy passed.
Windows used a separate native executable because the running app locked the standard executable.
Live visual checks and macOS checks did not run. Pilot entries and Xero data were unchanged by these tests.
