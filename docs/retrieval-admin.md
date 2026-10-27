# Entry retrieval and project administration

The development API and separate queue now support timed entries, project assignments, and remote retrieval.
The native app remains independent. No production identity, Xero connection, or deployment is added.

## Entry pages

`GET /v1/me/entries` requires inclusive `from_date` and `to_date` values.
The optional `limit` defaults to 50 and accepts 1 through 100.
Follow `next_cursor` with the same date bounds and limit. Null marks the last page.
Cursors bind to the authenticated tenant, person, and filters. They expire after 15 minutes.
An invalid or expired cursor returns 422. Restart the list when it expires.

Entries use UUID order. Pages show current data, not a fixed snapshot.
An entry created below the current position can appear only after a fresh list request.
Use the change feed for complete synchronization across concurrent changes.

## Change feed

`GET /v1/me/entry-changes` accepts `limit` and `cursor`.
Start without a cursor. Each response contains `items`, `next_cursor`, and `has_more`.
Each item contains an entry ID, version, deletion flag, and entry body. A deletion has a null entry body.
The feed exposes only the caller's own entries. It does not grant PM or admin access to another person's time.

Save `next_cursor` with the received records. Follow it while `has_more` is true.
Keep that cursor when the feed has no more records, then use it for later polling with the same limit.
Change checkpoints do not expire in this development service. An empty poll reuses the existing checkpoint.
Receipts, change records, and change checkpoints currently have no automatic retention limit.
Production retention, quotas, and reset recovery require a separate decision before shared rollout.

Each entry mutation locks its account's change counter before changing the entry.
It saves the entry, audit event, change record, and operation receipt in one transaction.
The lock keeps revision order consistent with commit order. A later commit cannot appear below an already-read revision.
Identical retries replay the receipt without adding another change record.
Migration 4 seeds the feed with each existing entry's current state, including deletions.
It does not invent earlier versions that were not retained by the feed.

## Project administration

The development adapter denies admin access by default.
For the single-person binary, set `TRACKFAST_DEV_PROJECT_ADMIN=1` to grant the configured identity project administration.
Test hosts can use `with_development_administrators` with configured tenant/person pairs.
The caller cannot grant a role through a request body or header.
This grant does not permit access to another person's entries or audit records.

| Endpoint | Behaviour |
| --- | --- |
| `GET /v1/admin/projects` | List all tenant projects in bounded pages. |
| `POST /v1/admin/projects` | Create a project with a stable client UUID. |
| `GET /v1/admin/projects/{project_id}` | Read details, member IDs, and an ETag. |
| `PUT /v1/admin/projects/{project_id}` | Replace details and membership with an If-Match precondition. |

Write fields are `name`, `colour`, `archived`, and `member_ids`. Creation also requires `project_id`.
Names must contain non-space text and at most 200 input characters. Saved names are trimmed.
Colours use six hexadecimal digits after `#`.
Membership has at most 1,000 unique IDs. Each ID must belong to a configured identity in this tenant.
The development binary configures one person. Production membership and identity provisioning remain separate work.
Set `archived` to true or false to archive or restore a project. Project deletion is not provided.

Mutations require an Idempotency-Key. Replacement also requires the last ETag in If-Match.
Details, membership, the admin audit event, and the operation receipt commit together.
Stale versions return 412. An identical accepted retry returns its original response.
Admin permission is checked before replay. Revoked admin access cannot replay an old response.
The assigned-project list continues to check membership on every page.

## TDD and validation

The new timed/project queue test failed before the fields and validation existed.
Entry pagination returned 422 before implementation. The admin permission test returned 404 before its routes existed.
The admin workflow and durable-checkpoint tests failed before their supporting methods existed.
The contract test failed before the new endpoint schemas existed.
A late-receipt test exposed loss of a newer pulled version. The fix retains that version after confirmation.
Additional HTTP recovery, concurrency, migration, and rollback checks passed when added.
No defect was introduced to force those checks to fail.

Tests use generated SQLite files and a disposable Postgres database with a separate schema for each API test.
The native app database and real integration accounts are not used.

Windows and Linux each passed 30 API tests and 17 queue tests.
Formatting, Clippy with warnings denied, and builds passed for both changed Rust crates on both platforms.
Windows also passed all 23 Python contract and demo-tool checks. These checks made no Xero requests.
The interface did not change, so no visual checks ran. Native app checks were not repeated in this step.
macOS was not compiled or tested.
