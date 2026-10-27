# Local operation queue and development transport

The separate `sync-client/` Rust library provides a durable queue and a loopback HTTP adapter.
It supports completed timed and duration entries with optional project assignments. Original seconds remain exact.
Response checks support rounding Off or five-minute rounding. The native app is not connected.
`Entry` stores the common fields, optional `project_id`, and optional `Timing`. `DurationEntry` remains a compatibility type name.
For timed work, the supplied original seconds must match the endpoints. The transport omits that field from the timed request.
Personal settings now work in the API. The client does not queue settings changes yet.

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
Migration 3 adds a remote checkpoint and retained remote conflicts. Old entry bodies decode with no project or timing fields.

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
`DevelopmentClient::read` retrieves the current remote timed or duration entry and version for review.
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

1. Complete a live Google flow through the Rust session client before connecting the native sign-in interface.
2. Add scheduled retries and the remaining conflict choices, including explicit acceptance of a remote deletion.
3. Agree on offline recording-time setting snapshots before connecting the native database worker and interface.
4. Add Xero exports and restricted PM reports before the pilot.

The approved rounding and date policies are recorded in `api-contract.md`.
Remote entry reads expose the original duration, effective duration, and applied interval.
This development adapter does not export to Xero or change export states.

## Remote retrieval

`list_entries` and `list_projects` return bounded pages. Follow their next cursors with unchanged filters and limits.
`pull_changes` retrieves one page of up to 100 entry changes and returns whether another page is available.
Call it again while more pages exist. Call it later to retrieve further changes. There is no scheduler in the library.
The client checks identity, response size, entry ownership, timed fields, and calculated totals before applying a page.

The queue saves the page and checkpoint in one SQLite transaction. A failed write retains the previous checkpoint.
Changes to confirmed entries replace the local confirmed copy. Remote deletions retain an ID record that prevents reuse.
Changes to entries with pending work retain both copies. `remote_conflict` exposes the remote copy or deletion for review.
A later successful receipt cannot discard a newer remote version already received through the feed.
The queue does not yet provide an accept-remote conflict action. Pending work remains available until explicit resolution.

See [retrieval and project administration](retrieval-admin.md) for the endpoint rules and validation record.

## Google pilot sessions

`session::SessionClient` adds secure refresh storage, automatic renewal before operations, and sign-out cleanup.
It wraps the existing loopback transport and keeps all queue receipt checks.
See [client sessions](client-sessions.md) for its public methods, platform requirements, and failure recovery.
The original `DevelopmentClient` still accepts an in-memory token for development tests.
