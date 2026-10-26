# API contract: phase 1

Status: target contract. A limited development implementation now exists in `server/`.
See [server scope](server.md) for supported operations and restrictions. The development queue now has a loopback HTTP adapter. The native app is not connected.

The machine-readable contract is [OpenAPI](../api/openapi.json).
Client examples are in [examples.json](../api/examples.json).
The [current architecture](architecture.md) describes the running native app.
The [roadmap](roadmap.md) describes the target product and delivery sequence.

## Product boundary

Trackfast has two deliverables: a usable API and a polished native app.
Build them independently against shared rules. Connect the native app after both work.
Internal teams can build TypeScript clients without native Rust code or private endpoints.
The server validates shared rules and owns accepted records. Clients retain pending offline work.
Production integration secrets stay on the server. The Xero demo tools are isolated development tools.

## Entries

Use stable UUIDs for entries and projects. Clients generate entry IDs before sending local work.
The authenticated identity supplies ownership. A request cannot select another owner or set an export state.
Use integer seconds for original and effective durations. Do not use floating-point hours for stored time.
The interface can accept hours and minutes, then convert them to seconds.

A duration-only entry has a selected date and original duration. It has no clock endpoints.
A timed entry has start and stop timestamps with explicit UTC offsets.
The server derives its original duration from those endpoints. The stop must be later than the start.
Phase 1 describes completed entries. Running-timer commands remain a later contract extension.
The three-row widget limit does not limit the number of stored entries.
Overlaps never divide or reduce an entry. A three-hour entry remains one entry.

Entry dates are calendar dates, not timestamps. Preserve a duration-only entry's selected date.
Timed entries keep their start date in the selected time zone. Do not split an entry at midnight.
Preserve that assigned date when the person later changes their time zone.

## Rounding and settings

Each person selects rounding independently. The default interval is five minutes. Null means Off.
With rounding enabled, use the rounded duration for totals and export.
With rounding disabled, use the exact original duration. Do not force clean totals on that person.
Always preserve the original duration and actual calendar positions.
The 09:03 to 09:16 example stores 780 original seconds.
With five-minute rounding it exposes 900 effective seconds. With rounding off it exposes 780 effective seconds.
Aggregate effective durations without another rounding pass.

The entry response includes its applied interval, original duration, and effective duration.
The server calculates effective duration. Clients cannot supply an authoritative effective duration.
The create draft uses the person's server setting. Offline setting snapshots require a decision before sync work.
Setting changes apply only to new entries. Existing entries keep their applied rounding interval.

Personal settings contain separate rounding, visual indicator, and weekly target fields.
The weekly target defaults to 35 hours. It does not cap, divide, or alter recorded time.
The visual indicator can be a week glass or simple progress display. Its initial default remains open.
For this draft, the rounding field accepts Off or 5. Further intervals remain open.
The schema requires a positive weekly target. An upper limit remains open.

Xero accepts whole-minute durations. Trackfast preserves seconds when rounding is off.
The treatment of sub-minute remainders during Xero export remains open. Do not silently truncate them.

The user approved these rules on October 26, 2026:

- Exact half-interval ties round up.
- Entries whose rounded duration is zero remain visible but are not exported.
- Setting changes apply only to new entries. Existing, exported, and invoiced entries retain their applied setting.

Offline setting snapshots and Xero sub-minute remainders still require decisions.

## Initial operations

| Operation | Purpose |
| --- | --- |
| `GET /v1/me` | Read the authenticated tenant and person before sync. |
| `GET /v1/projects` | List projects available for the person's own time entries. |
| `POST /v1/me/entries` | Create a completed entry with a stable client ID. |
| `GET /v1/me/entries` | List the person's entries in a bounded date range. |
| `GET /v1/me/entries/{entry_id}` | Retrieve one owned entry and its ETag. |
| `PUT /v1/me/entries/{entry_id}` | Replace an editable entry with an If-Match precondition. |
| `DELETE /v1/me/entries/{entry_id}` | Delete an editable entry with an If-Match precondition. |
| `GET /v1/me/settings` | Retrieve personal settings and their ETag. |
| `PUT /v1/me/settings` | Replace personal settings with an If-Match precondition. |

These endpoints form a proposed version 1 contract. The limited local implementation is not a deployed service.
Use bearer access tokens. Select the production identity provider and token lifecycle before shared deployment.
Google Workspace availability is unconfirmed. The local development adapter does not establish production membership.
Validate token signature, issuer, audience, and expiry. Derive tenant and person from the verified identity.
Do not trust a caller-supplied person ID, company domain, or role.
Return 404 for an entry outside the caller's scope. Do not disclose whether another person owns that ID.
Reject projects outside the caller's access and new time on archived projects with 422.
Date bounds are inclusive. Start must not exceed end.
List responses contain an opaque next cursor or null. Bind cursors to identity and filters.
The API must impose bounded page sizes. Cursor expiry and snapshot consistency need a sync design before phase 3.

## Proposed mutation and retry rules

Create, update, and delete requests require a UUID Idempotency-Key.
Scope keys to the authenticated person and organisation, across mutation endpoints.
Persist the key, normalized request, result, and business change in one database transaction.
Include the method, resource, and precondition in the stored request identity.
Replay an identical accepted request with its original status and result, without another write.
Return 409 idempotency_conflict when that key identifies a different request.
Check replay before evaluating a now-stale If-Match value.
This also allows a successful delete to replay its 204 response after the entry disappears.
Do not reuse a deleted entry ID to create a new entry.
Retain deletion records for sync. Decide key retention and deletion retention before offline integration.

Each accepted update increments the server version. ETags are quoted version numbers, for example `"1"`.
PUT and DELETE require If-Match. Return 428 when it is absent and 412 when the version changed.
The draft does not accept wildcard If-Match. A client must read the current version before replacing it.
Use one transaction for the entry change, audit record, and operation receipt.
Return 409 entry_locked when export restrictions prevent a mutation.
Do not silently overwrite a conflict or discard the client's unsent changes.

Errors contain a stable code, a safe message, and a request ID.
Never return credentials, provider authorization responses, or database details in errors.
Return 401 for missing or invalid authentication. Return 403 for a forbidden operation.
Return 422 for invalid field values. Cross-field rules require server tests in addition to schema validation.
Malformed JSON returns 400. Missing required Idempotency-Key returns 400.

## Roles and reporting

The personal endpoints expose only the caller's own entries and settings.
PM access does not permit reading developers' entry endpoints.
The later PM endpoint returns each developer's previous-week totals on the PM's assigned projects.
It must not return dates within the week, work times, notes, private targets, or totals for other projects.
The app presents a donut chart and durations from that restricted response.
Monday through Sunday in Africa/Johannesburg remains the proposed reporting boundary.
Admin mapping access does not automatically grant full audit access.
Audit-reader permissions and operational event fields remain proposals for review.

## Acceptance and TDD

Contract tests validate the OpenAPI document, resolve references, and exercise request and response examples.
Negative tests reject ownership injection, authoritative client totals, invalid dates, and mixed entry modes.
These tests do not prove authentication, persistence, arithmetic, or conflict behaviour in a running service.
Add each behavioural test before its implementation in phase 2A. The first Postgres workflow now has integration tests.

The first API workflow must prove:

1. An authenticated person creates and retrieves a duration-only entry in Postgres.
2. The entry remains after a server restart.
3. An identical create retry returns the same entry without another audit event.
4. The same key with different content fails without a write.
5. A valid replacement increments its version. A stale replacement preserves the saved entry.
6. A different person cannot read, change, delete, or list the entry.
7. Deletion and its replay preserve the operation receipt and deletion record.
8. Invalid requests change no records.

The native app must later pass the same shared entry examples.
Its local workflows also need visual checks, keyboard checks, and restart recovery tests.
Keep the desktop app on local storage during this phase.

Run contract checks with Python 3.12 or later:

```text
python -m pip install -r tools/requirements-contract.txt
python -B -m unittest discover -s tools -p test_api_contract.py
```

Use an isolated Python environment. These packages are test dependencies, not desktop runtime dependencies.
