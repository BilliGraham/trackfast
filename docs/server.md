# Development API

The standalone Rust service in `server/` uses axum and Postgres.
It does not change the native app or connect to Xero.
This is a development implementation of part of the target OpenAPI contract.

## Supported behaviour

- Create, retrieve, replace, and delete completed timed and duration-only entries.
- Derive elapsed seconds and validate the saved time zone for timed entries.
- List assigned projects with account-bound cursors and check access before saving project time.
- Read the authenticated tenant and person through `GET /v1/me` before sync.
- List the caller's entries within an inclusive date range containing at most 100 results.
- Preserve exact original seconds and calculate effective seconds with the entry's applied rounding interval.
- Read and replace personal rounding, weekly target, and visual indicator settings.
- Derive the owner from a configured development credential.
- Require an operation key for mutations and If-Match for replacement and deletion.
- Replay accepted operations without duplicate changes or audit events.
- Keep the entry change, operation receipt, and audit event in one transaction.
- Retain deletion records and reject reuse of a deleted entry ID for the same person.

Entries can be unassigned or belong to an active project assigned to the caller.
See [timed entries and project access](timed-projects.md). Entry-list pagination remains unfinished.
More than 100 matching entries returns a validation error. It never returns an incomplete list as a complete result.
Personal settings default to five-minute rounding, 35 weekly hours, and the progress indicator.
See [settings and rounding](settings.md) for the approved rules and migration behaviour.

## Local configuration

The binary requires these environment variables:

| Variable | Value |
| --- | --- |
| `TRACKFAST_DEV_MODE` | `1` |
| `TRACKFAST_DATABASE_URL` | Connection string for a dedicated local Postgres database. |
| `TRACKFAST_DEV_TOKEN` | A random development bearer token with at least 32 characters. |
| `TRACKFAST_DEV_TENANT_ID` | A stable UUID for the test organisation. |
| `TRACKFAST_DEV_PERSON_ID` | A stable UUID for the test person. |

Supply credentials through your local environment. Do not commit them or paste them into chat.
Use the same tenant and person IDs after restart to access the same entries.
The service binds only to `127.0.0.1:8788`. It rejects non-loopback Postgres destinations.
Local HTTP and the local Postgres connection do not use TLS. Do not expose this adapter through a proxy or tunnel.
The fixed token adapter is not Google sign-in. Token renewal, expiry, revocation, and production membership checks remain unimplemented.
Requests and errors never print tokens, database connection strings, or provider response bodies.

Run from the repository root:

```text
cargo run --manifest-path server/Cargo.toml --locked
```

Use `Authorization: Bearer <development token>` for requests.
Use a fresh UUID in `Idempotency-Key` for each new mutation. Keep that key for its retries.
Read the ETag before a replacement or deletion. Send it unchanged in If-Match.
Use the duration-only example in `api/examples.json` with a fresh entry ID.
The server implements personal settings and project listing. Project administration remains a later task.
The separate `sync-client/` library now uses this API. It checks the account before sending entries.
The native app is not connected. Production identity remains unconfigured.

## Storage and tests

Server migrations are in `server/migrations/` and the ordered `MIGRATIONS` list in `server/src/lib.rs`.
Append a migration for later schema changes. Do not edit released migrations.
Startup applies pending migrations in one transaction and rejects newer schema versions.
Each request uses an asynchronous Postgres connection. Connection pooling remains a later performance task.
The service applies bounded database connection, statement, and lock timeouts.

Set `TRACKFAST_TEST_DATABASE_URL` to a disposable Postgres database before running tests.
Tests create generated schemas and remove them after success. Failed tests can leave their schemas in that disposable database.
The test suite fails when the database variable is absent. It does not silently skip integration checks.
HTTP tests use the real router and Postgres through a loopback TCP listener.
They suppress the first successful create, edit, and delete response, reopen the queue, and retry each operation.
The database must contain one entry record and exactly three operation receipts and audit events.
Other checks cover invalid receipts, account mismatch, and explicit resolution after a conflicting remote edit.

```text
cargo fmt --manifest-path server/Cargo.toml --all --check
cargo clippy --manifest-path server/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path server/Cargo.toml --locked --all-targets
cargo build --manifest-path server/Cargo.toml --locked
```

TDD record: the initial three integration tests failed against a stub with HTTP 501.
The implemented handlers then passed all three against Postgres.
The concurrency and rollback tests passed on their first run. No defect was introduced to force a failure.
A new strict-date test returned HTTP 200 instead of 422 before the date validation fix.

Initial phase 2A validation on October 26, 2026:

- Windows: six API integration tests passed against Postgres 17. Formatting, Clippy, and build passed.
- Linux container: six API integration tests and build passed against Postgres 17.
- Windows HTTP smoke check: creation, process restart, retrieval, and replay of the accepted create all passed.
- Windows desktop regression: 51 Rust tests, formatting, Clippy, and build passed.
- Windows tooling: nine contract checks and ten Xero-tool checks passed.
- macOS: no checks ran. No visual interface checks were needed for this server change.

The disposable validation database was removed after these checks. No development service was left running.

Production authentication, deployment, rate limiting, scheduled sync, and export jobs remain unimplemented.
