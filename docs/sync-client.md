# Local operation queue and development transport

The separate `sync-client/` Rust library provides a durable queue and a loopback HTTP adapter.
It supports unassigned duration entries with exact recorded seconds and rounding Off.
The native app is not connected. Timed entries, projects, and settings require further API work.

## Storage rules

- Save the local entry and its pending operation in one SQLite transaction.
- Generate each operation UUID once. Preserve the key, content, and expected version across retries and restarts.
- Bind the queue to one server, tenant, and person. Reject a different scope.
- Use a separate database file. Reject unrelated databases and newer schema versions.
- Confirm the server version and remove the pending operation in one transaction.
- Keep deletion records so a deleted entry ID cannot be reused.

Only one pending operation is allowed per entry. A further edit waits for confirmation or conflict resolution.
An edit immediately saves its local values. A pending deletion retains the local entry until server confirmation.
Migration 2 adds versions, deletion records, and conflict state. It preserves pending work from migration 1.

The caller must run database and HTTP work on a worker thread.
The library starts no worker and has no interface dependency.
Use a private application data directory. SQLite stores entry text without encryption, as the local app does.
Do not put credentials in the scope or file path. The transport token remains in memory.

## HTTP delivery

`DevelopmentClient::new` accepts a canonical HTTP address with a loopback IP and explicit port.
For example, use `http://127.0.0.1:8788`. Hostnames, remote addresses, URL credentials, paths, queries, and fragments are rejected.
The adapter disables proxies and redirects. It limits request time and response size.
It does not provide production authentication or TLS. Do not expose the development API through a tunnel.

`sync` checks `GET /v1/me` against the queue's tenant and person before it sends entries.
It sends each stored operation key as `Idempotency-Key`.
Edits and deletions also send the stored version as `If-Match`.
The client checks success status, entry content, ownership, version, and ETag before confirming a create or edit.
A deletion requires the expected successful no-content response for that request.

Failed authentication, invalid responses, conflicts, timeouts, and server errors keep pending work.
A sync pass stops at its first failure. Earlier confirmed operations remain confirmed.
The caller controls retry timing. There is no automatic retry loop or background scheduler yet.

## Conflict handling

A version conflict records conflict state and keeps the local values and original operation key.
`DevelopmentClient::read` retrieves the current remote duration entry and version for review.
After the user chooses the local values, the caller can use `Queue::resolve_with_local` with the reviewed remote version.
This explicit action creates a new operation key and updates the expected version.
If another client changes the entry again, the next request conflicts again.
There is no automatic overwrite, merge, or native conflict dialog yet.
Creation conflicts and remotely deleted entries remain pending for a later recovery workflow.

## Checks

Run these commands from the repository root:

```text
cargo fmt --manifest-path sync-client/Cargo.toml --all --check
cargo clippy --manifest-path sync-client/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path sync-client/Cargo.toml --locked --all-targets
cargo build --manifest-path sync-client/Cargo.toml --locked
```

Run the server suite with a disposable Postgres database to test the transport over HTTP.
See `server.md` for the required environment variable and commands.
Tests use generated SQLite files or in-memory databases. They never open the native app database.

TDD checks failed before implementation for identity discovery, destination validation, HTTP recovery, queued changes, remote inspection, and explicit conflict resolution.
Additional conflict retention, receipt validation, migration, and transaction rollback checks passed without another implementation change.
Mutation recovery tests suppress successful responses, reopen the queue, and retry with unchanged operation keys.
They check the real Postgres entry, operation, and audit counts.

On October 26, 2026, all 13 queue tests and all 10 server tests passed on Windows and Linux.
Both crates built on both platforms. Linux checks ran in a disposable Rust 1.98 container.
Windows formatting and Clippy checks passed for both crates.
All 51 native-app tests and all 20 Python checks passed on Windows.
macOS and visual checks did not run. The interface did not change.

## Remaining work

1. Complete timed entries, projects, settings, list pagination, and shared entry rules in the API.
2. Select and configure production identity. Google Workspace availability is unconfirmed.
3. Add scheduled retries, remote change retrieval, and the remaining conflict choices.
4. Connect the native database worker and interface after the required API operations work.
5. Add Xero exports and restricted PM reports before the pilot.

The approved rounding and date policies are recorded in `api-contract.md`.
This development adapter still uses rounding Off. It does not export to Xero or change export states.
