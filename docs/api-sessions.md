# Local Google sessions

Google verifies identity. Trackfast stores membership, roles, and sessions in its own Postgres database.
The native app still uses local SQLite. This step does not connect the native app or deploy a service.

## Local setup

Use a separate local Postgres database. Do not use a production database for pilot checks.
The API listener and database must use loopback addresses. HTTP is permitted only for this local pilot.
Google requests use HTTPS. Shared hosting, TLS termination, and operational controls require a separate task.

Set these variables in a new PowerShell window:

```powershell
$env:TRACKFAST_LOCAL_MODE='1'
$env:TRACKFAST_AUTH_MODE='google'
$env:TRACKFAST_DATABASE_URL='postgres://YOUR_LOCAL_USER:YOUR_LOCAL_PASSWORD@127.0.0.1:YOUR_PORT/YOUR_PILOT_DATABASE'
$env:TRACKFAST_GOOGLE_CLIENT_ID='YOUR_DESKTOP_CLIENT_ID'
```

Do not set `TRACKFAST_DEV_MODE` or `TRACKFAST_DEV_TOKEN` in this window.
Google mode rejects these variables. Development tokens cannot authenticate in Google mode.

An operator approves each Google subject before sign-in. Use the subject from the successful [Google check](google-signin.md).
Select permanent tenant and person UUIDs. Use the existing IDs if this person already has API data.
Keep these IDs unchanged across restarts and clients. Never use the native SQLite IDs as API ownership IDs without an explicit migration.

```powershell
cargo run --locked --manifest-path server/Cargo.toml -- provision-google-member GOOGLE_SUBJECT TENANT_UUID PERSON_UUID member
cargo run --locked --manifest-path server/Cargo.toml
```

Use `admin` instead of `member` only to grant project administration explicitly.
Provisioning inserts a new membership. Repeating the command does not change an existing role or enable a disabled account.
The command rejects a subject already bound to different IDs.
Role editing, account reactivation, and a membership administration interface remain later work.

In another window, run the session check:

```powershell
& "$env:TEMP\trackfast-signin-venv\Scripts\python.exe" -B tools/api_signin.py --client-id YOUR_DESKTOP_CLIENT_ID --email YOUR_APPROVED_EMAIL
```

Install `tools/requirements-signin.txt` in that environment first. See the Google check instructions for environment creation.
Enter the desktop client secret at the hidden prompt if Google supplied one.
The tool obtains a server nonce, completes browser sign-in, and requests an API session.
It checks identity, renews the session, signs out, and checks that access is rejected.
It does not create or change time entries. No tokens are printed or saved.
The email argument checks the selected account locally. Server membership uses the verified Google subject, not this argument.

To disable a member, use the same Google mode configuration:

```powershell
cargo run --locked --manifest-path server/Cargo.toml -- disable-google-member GOOGLE_SUBJECT
```

This command disables sign-in and revokes all sessions for the subject. Time records remain unchanged.
Requests that already passed authorization can finish. Later authorization checks reject the disabled member.
No HTTP endpoint lets a caller grant membership or change their role.

## Protocol

The [OpenAPI file](../api/openapi.json) defines the payloads.

| Endpoint | Behaviour |
| --- | --- |
| `POST /v1/auth/google/challenge` | Return a nonce and challenge ID. The challenge expires after five minutes. |
| `POST /v1/auth/google/exchange` | Consume the challenge and verify the signed Google ID token. Return a Trackfast session for an enabled member. |
| `POST /v1/auth/refresh` | Replace the access token and refresh token. Reuse of an old refresh token revokes the session. |
| `POST /v1/auth/logout` | Revoke the session identified by the supplied refresh token. |
| `GET /v1/me` | Return the tenant and person bound to the current session. |

An exchange attempt consumes its challenge even if verification fails. Start with a new challenge after any failed attempt.
Google RS256 signatures, issuer, audience, authorized party, token times, verified-email flag, subject, and nonce must pass verification.
Both documented Google issuer spellings map to the single Google provider. The server never accepts other issuers.
Neither an email suffix nor a Workspace claim grants access. Roles and ownership come only from stored membership.
Google ID tokens cannot serve as Trackfast bearer tokens.

Access tokens last at most 15 minutes. A session lasts at most seven days from its initial creation.
Renewal never extends that final expiry. `expires_in` reports the shorter remaining lifetime when necessary.
Tokens contain 244 random bits from two UUID v4 values. Only SHA-256 hashes are stored.
Each refresh transaction locks the session. Two concurrent refresh requests cannot both succeed.
Clients must serialize renewal and replace both tokens together.
If a renewal response is lost, sign in again. Do not retry the old refresh token.
The old token remains recorded until session expiry so reuse can revoke the session.
Expired sessions are removed during later successful sign-ins. There is no scheduled cleanup service yet.

Google signing keys come only from the fixed Google HTTPS endpoint.
Requests have size and time limits. Redirects and proxies are disabled.
The key cache respects `max-age` up to one hour. Unknown keys can trigger one refresh per 30 seconds.
Verification fails closed when a required key cannot be obtained.
At most 256 unexpired challenges can exist. This bound is not a shared-service abuse protection system.

## Validation and limits

The first HTTP test failed with 404 before the auth routes existed.
The signed-token and lifecycle tests first failed to compile before their implementation existed.
The lifetime regression failed because renewal reported 900 seconds near final expiry. The correction returns the remaining lifetime.
The Python tool and OpenAPI checks also failed before their implementations existed.

Tests use a generated RSA key with no Google or Trackfast account access.
The test key is public fixture material. It is never used by the executable or accepted by the production verifier.
Tests cover invalid claims and signatures, unapproved subjects, replay, expiry, renewal, concurrent renewal, sign-out, disabled membership, and user isolation.
Existing database, project, entry, and queue tests remain required.

Validation completed on October 27, 2026:

| Platform | Result |
| --- | --- |
| Windows | API formatting, Clippy, build, and 33 tests passed. Queue checks and 17 tests passed. Native checks and 67 tests passed. |
| Linux | API formatting, Clippy, build, and 33 tests passed. Queue checks and 17 tests passed. |
| Windows and Linux | All 32 Python contract and sign-in tests passed. |
| macOS | Not compiled or tested. |

No interface changed. No visual check was required.

The user reported a successful live API session check on Windows on October 27, 2026.
The API accepted the approved Google identity. Renewal preserved the tenant and person. Sign-out revoked API access.
The check saved no Google or session credentials and changed no time entries or Xero records.
This result covers one approved account on the local pilot. It does not verify shared deployment or live access between different accounts.
Automated tests do not prove deployment security. Before shared access, add TLS, abuse controls, operational monitoring, and a reviewed membership administration process.
Secure client token storage and the native sign-in interface remain later work.
References: [Google OpenID Connect](https://developers.google.com/identity/openid-connect/openid-connect)
and [jsonwebtoken](https://docs.rs/jsonwebtoken/10.4.0/jsonwebtoken/).
