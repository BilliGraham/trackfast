# Native remote sync

**Start delivery** now sends new local work and receives entries from the configured Trackfast API.
This includes API entries from another client for the same tenant and person.
Local history from before setup and Toggl imports remain excluded from uploads.
API history can be downloaded. Local settings remain separate from API settings.
This step does not export to Xero or deploy a shared service.

## Apply remote changes

The account worker reads at most 100 changes in each network cycle.
It saves the batch in the queue before asking the native database worker to apply any item.
The native worker applies one item in one transaction, then publishes a new interface snapshot.

- Clean entries accept API creation, correction, and deletion.
- Unsent local edits or deletions enter [conflict review](native-conflicts.md). Neither version silently replaces the other.
- Entries require explicit local project mappings. A missing mapping pauses delivery and shows the API project ID.
- Exactly one reverse mapping must exist unless the current local project already matches. Ambiguous mappings also pause application.
- Downloaded entries retain original seconds, applied rounding, saved dates, and time-zone information.

For a missing mapping, create or select the local project, refresh API projects, and save the mapping in Settings.
Then select **Start delivery** again. The saved batch remains available.
Mapping reassignment and a selector for ambiguous reverse mappings remain later work.

An older API entry can omit its recording-time settings.
The app displays its applied rounding but preserves the absent recording snapshot in later API edits.
Thus, a description change does not silently change the API rounding policy.

## Restart and retry

Each downloaded item has a stable token. The native transaction saves the token, a content fingerprint, and its result.
If the queue acknowledgement fails, replay returns that result without applying the entry again.
A local edit made after the first application therefore remains pending.

The queue updates the confirmed baseline only after native acknowledgement.
It advances the download cursor only after every item in the saved batch is acknowledged or already confirmed at a newer version.
Previously confirmed local writes are skipped when they appear in the change feed.
A downloaded entry is marked confirmed locally, so it is not uploaded as a new entry.

A frozen outbound transfer finishes before saved downloads are applied.
Saved downloads then finish before another outbound transfer is prepared.
This order preserves receipt recovery and prevents a stale download from silently replacing newer local work.

Network failures use persistent retry delays. Server retry deadlines remain enforced after restart.
Conflicts, invalid responses, and storage failures pause delivery with an error.
Stop delivery and sign-out stop subsequent work. An operation already in progress can finish.
Delivery requires another explicit start after app restart.

## Validation

TDD checks first failed for missing native application, controller events, transport download, and mapping diagnostics.
Tests cover create, edit, delete, stale versions, legacy rounding, local-edit races, missing mappings, and replay.
A failing copy test found lost rounding after both versions were deleted. The saved applied rule now restores the correct copy total.
Queue tests cover 100-item batches, a second page, cursor retention, conflicts, and skipped local-write echoes.
Controller tests check stopped delivery and receipt priority over downloaded work.
The HTTP test uses a disposable Postgres database and a generated queue file.
It checks account isolation, saved progress after reopen, and unchanged entry, operation, and audit counts during download.

Live pilot checks remain next. Use a test entry to check changes from a second client, a competing edit, and restart recovery.

On October 29, 2026, Windows and Linux passed 117 native tests, 59 queue library tests, one queue example test, and 37 API tests.
The native total includes all seven timer tests. All three builds passed on both platforms.
Windows formatting and Clippy checks passed. Automated layout checks cover narrow views and both themes.
The optional operating-system credential-store test did not run. Live visual checks and macOS checks did not run.
The tests did not change pilot entries or Xero records.
