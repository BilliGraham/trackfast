# Client session handling

`sync-client::session::SessionClient` provides reusable session handling for the local pilot API.
It uses the existing queue transport. The native app remains separate and still uses local SQLite.
The client runs no background service. Call its methods from a worker thread.

## Public operations

| Method | Behaviour |
| --- | --- |
| `SessionClient::local(scope)` | Open the account credential and acquire its local lock. No HTTP request occurs. |
| `challenge()` | Obtain the API nonce for a Google sign-in attempt. |
| `sign_in(challenge_id, id_token)` | Exchange the Google ID token, check ownership, and store the refresh credential. |
| `check_identity()` | Check API access and renew the session when necessary. |
| `sync(queue)` | Deliver pending operations with the existing receipt and ownership checks. |
| `read(id)` | Read an entry owned by the current person. |
| `list_entries(...)`, `list_projects(...)` | Retrieve bounded pages with the existing cursor checks. |
| `pull_changes(queue)` | Retrieve one change page and save it with its checkpoint. |
| `sign_out()` | Block local access, revoke the session, and delete the stored credential. |

The scope contains the canonical API address, tenant UUID, and person UUID.
Use the IDs from the approved API membership. Changing the person, tenant, or server selects a different credential.
The client checks the verified API identity before it enables access. It rejects a different queue scope before sending entries.

The caller still owns the Google browser flow. Use the challenge nonce, a random state, and PKCE.
Validate the callback state before passing the Google ID token to `sign_in`.
The client does not retain Google credentials. A new sign-in cannot silently replace an existing session.
Complete sign-out cleanup first.

```rust,no_run
use trackfast_sync::{Scope, session::SessionClient};

fn check_account(scope: Scope) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = SessionClient::local(scope)?;
    client.check_identity()?;
    Ok(())
}
```

This example restores an existing credential. It reports `SignInRequired` when no usable session exists.
Dropping the client releases its lock. It does not sign out or remove queued work.

## Credential storage

Refresh tokens use Windows Credential Manager, Linux Secret Service, or macOS Keychain.
Linux uses encrypted Secret Service transport. The library requires an available, unlocked credential store.
Storage errors stop the operation. There is no plaintext file or SQLite fallback.
Unsupported operating systems also fail closed.

Only the refresh token, account scope, format version, and recovery state enter the credential store.
Access tokens remain in memory. Token types do not implement `Debug`.
Owned token buffers use zeroization when discarded. This does not erase every copy in network or operating system buffers.
Errors contain fixed messages and never include provider responses or token values.

The local application-data directory contains a `session-locks` folder.
Each account has an empty `.lock` file and, during an incomplete operation, an empty `.blocked` file.
File names use a hash of the server and account IDs. Neither file contains credentials or time entries.
Keep these files in place. Deleting a lock file can defeat coordination between processes.
One client instance owns the credential at a time. A second instance receives `Busy`.
Other applications can use their own sessions and credential stores instead of sharing this credential.

## Renewal and failure recovery

The client renews before the next operation when the access token reaches its expiry margin.
The margin is 30 seconds. A monotonic clock measures the current process lifetime.
A restored client has no access token and renews once before its first authenticated operation.
Renewal does not run on a timer while the client is idle.

Before sending a refresh request, the client records a blocked state in the credential store and the local marker.
After a valid response, it retains the new refresh token before checking identity.
It clears the marker only after it saves a ready credential successfully.
An interrupted or ambiguous refresh never causes an automatic retry with the old token.
The caller must retry `sign_out`, then complete a fresh browser sign-in.

A protected request that returns 401 permits one renewal and one retry.
A second 401 blocks the session. Permission denials, conflicts, and network errors do not trigger renewal retries.
Queue operations keep their original IDs, content, and expected versions across an authentication retry.
The queue retains pending work when delivery fails.

Sign-out removes the in-memory access token first.
If the API is unavailable, the client retains a blocked refresh credential solely for revocation.
The marker also prevents restoration when the credential-store update failed.
Retry sign-out when the failed service is available. An already-invalid refresh token counts as revoked.
Sign-out succeeds only after remote revocation and local credential deletion.
It never deletes time entries or pending queue operations.

If the local marker cannot be written, the client reports a storage failure and still attempts remote revocation.
Do not treat any failed sign-out as complete. Retry cleanup before another sign-in.
If the initial exchange response is lost, the client has no refresh token to revoke. The server session remains subject to its expiry.
The next sign-in requires a new challenge. There is no automatic exchange retry.

## Checks

The first state tests failed before implementation because the session types did not exist.
Further failing tests covered 403 classification, cleanup after invalid responses, and restart protection during credential-store failure.
HTTP tests cover response limits, redirect rejection, renewal, ownership, and preservation of queue operation keys.
State tests cover expiry, restart, lost responses, storage failures, bounded authentication retries, and offline sign-out.

Validation completed on October 27, 2026:

| Platform | Result |
| --- | --- |
| Windows | Formatting, Clippy, tests, and builds passed for the native app, queue client, and API. |
| Linux | Formatting, Clippy, tests, and builds passed for the queue client and API. |
| Windows and Linux | All 33 queue tests and 33 API tests passed. The native credential-store test also passed separately. |
| Windows native app | All 67 existing tests passed. No interface changed. |
| macOS | The Keychain backend is configured. Compilation and tests did not run. |

The Linux credential-store check used an isolated session bus and disposable Secret Service inside the test container.
The Windows check used Windows Credential Manager. Both checks removed their synthetic credentials.

The native credential-store test uses a generated account and synthetic tokens. It removes the test credential afterward.
Run it separately because CI environments do not always have an unlocked credential store:

```text
cargo test --locked --manifest-path sync-client/Cargo.toml native_vault_roundtrip -- --ignored
```

The live Google-to-API check is recorded in [API sessions](api-sessions.md).
That earlier check used the Python tool. The subsequent live Rust check also passed on Windows.
The client currently accepts only canonical loopback HTTP addresses. Shared deployment and HTTPS configuration remain later work.
The native sign-in interface, retry scheduling, and remaining conflict choices also remain later work.

## Live Rust check

Build the example, then run the Python browser helper from the repository root:

```powershell
cargo build --locked --manifest-path sync-client/Cargo.toml --example session_check
& "$env:TEMP\trackfast-signin-venv\Scripts\python.exe" -B tools/rust_signin.py --client sync-client/target/debug/examples/session_check.exe --client-id YOUR_DESKTOP_CLIENT_ID --email YOUR_APPROVED_EMAIL --tenant YOUR_TENANT_UUID --person YOUR_PERSON_UUID
```

Use the existing membership IDs and local API configuration from [API sessions](api-sessions.md).
On Linux, the executable is `sync-client/target/debug/examples/session_check`.
The helper uses the same Python requirements as the earlier Google check.
Enter the desktop client secret at its hidden prompt. Complete the browser sign-in with the approved account.
The helper validates Google's response and passes the identity token through the Rust process's standard-input pipe.
It never puts tokens in command arguments, files, or terminal output.

The Rust example performs the API exchange and stores the refresh credential through `SessionClient`.
It drops that client and opens a new client instance. The new instance restores the credential and renews the session.
The check then signs out and checks that another client has no stored access.
It does not create or change time entries or Xero records.
The credential exists temporarily in the operating system store and is removed on success.

If the check fails or stops after sign-in, repeat the helper command with `--cleanup`.
Cleanup revokes and removes the stored session for that scope. It does not require Google sign-in.
The normal check refuses to replace an existing session. Cleanup is an explicit separate operation.

TDD checks failed before the Python bridge and Rust input reader existed.
The bridge tests cover the private input pipe, invalid challenges, and private error output.
A separate Windows test uses the real Rust executable and credential store with a synthetic local HTTP server.
That test passed. It proves the bridge and storage flow, not live Google authorization.
The Rust example passed formatting, Clippy, compilation, and its input test on Windows and Linux.
All 33 regular queue tests passed on both platforms. The three Python bridge tests passed on Windows.
The user reported a successful live Rust check on Windows on October 27, 2026.
The Rust client accepted the approved Google identity and stored the refresh credential.
A new client instance restored that credential and renewed the session.
Sign-out completed and the stored credential was removed. No time entries or Xero records changed.
This check covers one approved account and restoration in a new client instance. It does not test a system restart or shared deployment.

References: [keyring credential stores](https://docs.rs/keyring/3.6.3/keyring/)
and [Rust file locking](https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock).
