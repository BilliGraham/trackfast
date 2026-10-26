# Local operation queue

Phase 3 starts with `sync-client/`, a separate Rust library.
It does not send HTTP requests or change the native app.
The first step supports immutable, duration-only entries with exact recorded seconds.
It does not split long entries or apply rounding.

## Storage rules

- Save the local entry and its pending operation in one SQLite transaction.
- Generate the operation UUID once. Keep this key and the entry contents across retries and restarts.
- Keep the queue bound to one server, tenant, and person. Reject a different scope.
- Use a separate database file. Reject unrelated databases and newer schema versions.
- Keep the local entry after confirmation. Remove only the confirmed pending operation.
- Reject replacement of an existing entry. Edits require a later versioned operation implementation.

Only the future database worker must open and use this queue.
The library has no interface dependency and starts no worker itself.
The caller must supply a server identifier without credentials. Do not put tokens in the scope or database path.
SQLite stores entry text without encryption, as the local app does. Use the private application data directory.

The transport must check the authenticated tenant and person before it sends queued data.
It must check the response identity, entry fields, and operation receipt before it calls `confirm`.
An HTTP timeout, failed response, or conflict must not call `confirm`.
The queue alone cannot prove that a remote server accepted an operation.

## Checks

Run these commands from the repository root:

```text
cargo fmt --manifest-path sync-client/Cargo.toml --all --check
cargo clippy --manifest-path sync-client/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path sync-client/Cargo.toml --locked --all-targets
cargo build --manifest-path sync-client/Cargo.toml --locked
```

The first six tests failed before implementation. They passed after implementation.
Additional checks cover database isolation and confirmation of one operation among several.
Those two checks passed without another implementation change.
Tests use in-memory SQLite or generated temporary files. They do not open the app database.

On October 26, 2026, all eight queue tests and builds passed on Windows and Linux.
Linux checks ran in a disposable Rust 1.98 container.
Windows formatting and Clippy checks passed. All 51 native-app tests passed on Windows.
macOS checks and visual checks did not run for this change. The interface did not change.

## Next steps

1. Add authenticated identity discovery and a transport that checks server receipts.
2. Test response loss with the real development API. Retry with the same key and contents.
3. Add versioned edits, deletion, and explicit conflict handling without loss of local work.
4. Complete the API support for timed entries, projects, settings, and production identity.
5. Connect the native database worker and interface after the required API operations work.

Rounding policies that remain open in `api-contract.md` remain open here.
This library does not mark work as exported or logged in Xero.
