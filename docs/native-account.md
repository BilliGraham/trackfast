# Native pilot account connection

The native Settings screen now supports Google sign-in to the local Trackfast API.
It can restore a saved session, check the connection, cancel browser sign-in, and sign out.
This step connects the account only. The app does not send time entries, projects, or settings to the API.
The first-sync boundary is not created by sign-in. The later sync connection will leave existing history local, as agreed.

## Configure the local pilot

Keep the existing local API running in Google mode. Use its approved tenant and person IDs.
Create a private local JSON file with these public configuration fields:

```json
{
  "server": "http://127.0.0.1:8788",
  "tenant_id": "00000000-0000-4000-8000-000000000001",
  "person_id": "00000000-0000-4000-8000-000000000002",
  "google_client_id": "YOUR-DESKTOP-CLIENT.apps.googleusercontent.com"
}
```

Replace the example values with the pilot registration. Do not add a password, client secret, or token to this file.
The parser rejects unknown fields, invalid account IDs, and remote API addresses. The file has a 4 KiB limit.
Set `TRACKFAST_ACCOUNT_CONFIG` to its absolute path before starting the app.
Without this variable, the app retains local tracking and shows that account sign-in is not configured.

On Windows:

```powershell
$env:TRACKFAST_ACCOUNT_CONFIG = 'C:\path\to\native-account.json'
cargo run --locked
```

On Linux:

```sh
TRACKFAST_ACCOUNT_CONFIG=/absolute/path/native-account.json cargo run --locked
```

Linux requires an available, unlocked Secret Service. Windows uses Credential Manager. The macOS backend uses Keychain.
Only one client can own this account credential at a time. Close a standalone sign-in check before opening the native account.

## Check the connection

1. Open Settings in Trackfast.
2. Enter the Google desktop client secret if Google supplied one for this registration.
3. Select **Sign in with Google**.
4. Complete sign-in in the system browser with the approved account.
5. Return to Trackfast. The account card must show **Connected to Trackfast**.
6. Close and reopen the app with the same configuration. It must restore the session.
7. Select **Sign out**. The card must return to the signed-out state.

The desktop client secret is used only for this attempt. The app clears the field and its undo history when the attempt starts.
An invalid secret produces a fixed error message. The app does not log the provider response.
If sign-out fails, select **Retry sign-out** after the failed service becomes available.
Closing the app preserves its stored session. Use **Sign out** to revoke the session and remove the stored refresh credential.
The account card does not indicate whether entry sync is enabled. Entry sync is not enabled in this step.

## Implementation and limits

`src/account.rs` owns a dedicated account worker. It reads configuration and uses `SessionClient` outside the interface thread.
`src/account/oauth.rs` binds an ephemeral IPv4 loopback listener and creates random state and a PKCE verifier for each attempt.
The authorization request uses S256, the API challenge nonce, and only `openid email` scopes.
The callback checks its path, method, Host header, state, and unique code. It limits request size and waiting time.
Wrong-state callbacks are rejected without ending the valid attempt. A valid declined-consent callback ends the attempt.
Cancellation closes the listener. Network requests already in progress remain subject to their transport timeouts.
Cancellation after the API accepts sign-in attempts sign-out cleanup before reporting completion.

The Google token exchange uses its fixed HTTPS endpoint with proxies and redirects disabled.
The API validates the Google signature, issuer, audience, token times, challenge nonce, and approved membership.
The session client checks the returned tenant and person before enabling access.
Refresh credentials use the existing operating-system store. Access tokens remain in memory.
Secret input, state, PKCE verifier, callback buffers, and identity-token buffers use zeroization where owned by this code.
This does not erase all copies inside the network stack, browser, or operating system.

The native interface cannot override ownership or create a membership. Shared hosting and HTTPS API configuration remain later work.
No new desktop database migration is needed for this account-only step.

Reference: [Google OAuth for desktop apps](https://developers.google.com/identity/protocols/oauth2/native-app).

## Validation

The first tests failed before the native account types and callback flow existed.
The consent-refusal test failed because the listener kept waiting. The correction ends that attempt immediately.
Tests cover callback rejection, PKCE, cancellation, narrow account layout, duplicate clicks, and secret undo history.
The existing session suite covers restoration, renewal, identity binding, revocation, and credential-store failures.
Live Google sign-in from the native interface still requires the user's browser check.

On October 27, 2026, formatting, Clippy, tests, and builds passed for the native app on Windows and Linux.
Both platforms passed 76 native tests. The session library passed 48 library tests and one example test with the native dependency versions on both platforms.
The optional native credential-store test did not run in this step. Its earlier live checks remain recorded in `client-sessions.md`.
The Windows visual helper failed during initialization and after reset. Native visual checks did not run.
The account layout test passed at widths of 340 and 600 pixels. macOS compilation and tests did not run.
