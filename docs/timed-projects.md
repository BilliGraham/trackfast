# Timed entries and project access

The development API accepts completed timed entries and duration-only entries.
The native app and the separate sync queue are not connected to the new timed-entry or project features yet.
The sync queue still sends unassigned duration entries only.

## Timed entries

A timed request supplies `kind: timed`, `started_at`, `stopped_at`, `time_zone`, and `entry_date`.
It also supplies the description, optional project ID, and a new entry UUID on creation.
Do not send `original_seconds` or `effective_seconds`. The server calculates them.

```json
{
  "entry_id": "00000000-0000-4000-8000-000000000001",
  "kind": "timed",
  "entry_date": "2026-10-26",
  "project_id": null,
  "description": "Development",
  "started_at": "2026-10-26T09:03:00+02:00",
  "stopped_at": "2026-10-26T09:16:00+02:00",
  "time_zone": "Africa/Johannesburg"
}
```

This entry records 780 seconds. Five-minute rounding produces 900 effective seconds. Rounding Off preserves 780 effective seconds.

Timestamps must include known UTC offsets and represent whole seconds.
The offsets must match the selected time zone at each instant.
The entry date must equal the local start date in that zone. The stop must be later than the start.
Invalid zones, mismatched offsets, fractional-second instants, and unknown `-00:00` offsets are rejected.

The API stores the selected zone with the entry. Later profile changes cannot silently reassign its date.
Entries across midnight remain one entry. Elapsed seconds use actual instants across daylight-saving changes.
For example, New York's repeated 01:30 hour can represent an elapsed hour when the endpoints have different valid offsets.
Running timers remain outside this completed-entry contract.

The server uses [chrono-tz](https://docs.rs/chrono-tz/latest/chrono_tz/) for IANA time-zone rules on all supported platforms.
The locked dependency supplies the zone data. It does not depend on the host's selected time zone.

## Projects

`GET /v1/projects` returns projects assigned to the authenticated person in their tenant.
The response includes each project's UUID, name, colour, and archived flag.
An unassigned entry still uses `project_id: null`.

A new entry with a project requires an active project and an explicit membership row for that person.
Project checks run inside the entry transaction. Shared row locks prevent a concurrent membership removal or archive change from bypassing the check.
Unknown, inaccessible, and archived targets return the same validation status without disclosing project details.

On an archived project, an existing member can correct a note but cannot change the recorded work.
Changes to the date, duration, timestamps, time zone, or entry kind are rejected on that project.
A person can move their own entry to an active accessible project or leave it unassigned.
After membership removal, they can still retrieve or delete their own history. Further edits that keep the inaccessible project are rejected.
An identical accepted retry still returns its stored receipt without another mutation.

Projects and membership rows are provisioned through trusted database administration in this development phase.
There is no public project creation, membership grant, or administration endpoint yet.
This membership check does not implement production sign-in or PM reporting permissions.

## Project pagination

The default `limit` is 50. Accepted limits range from 1 through 100.
Follow `next_cursor` with the same limit. A null cursor marks the last page.
Each opaque cursor is bound to the authenticated tenant, person, and page size. It expires after 15 minutes.
Invalid, expired, or mismatched cursors return 422. Restart the list without a cursor when needed.

Pages use project UUID order and check current membership on every request.
They are a live list, not a snapshot. A project added below the current position appears on the next complete refresh.
Archived assigned projects remain in the list so clients can identify them as archived.
Entry-list pagination remains separate unfinished work.

## TDD evidence

The valid timed-entry tests returned 422 before implementation. They now test the expected successful responses.
The project-list test returned 404 before its endpoint existed. The access test initially lacked project storage.
Tests cover project membership, archived projects, tenant isolation, cursor scope, expiry, and membership removal between pages.
Timed tests cover retry results, exact and rounded totals, invalid inputs, midnight, and both daylight-saving transitions.
Negative timed-input tests passed before implementation because the old API rejected all timed entries. No defect was introduced to force a failure.

## Validation

Windows checks passed: formatting, Clippy with warnings denied, tests, and builds for the native app, server, and sync queue.
The test totals were 51 native app and timer tests, 24 server tests, 14 queue tests, and 22 Python checks.
Linux container checks passed: 24 server tests, 14 queue tests, and builds for both crates.
Server tests used a disposable Postgres database with a separate schema for each test.
The native app was not tested on Linux in this step. macOS and visual checks were not run.
