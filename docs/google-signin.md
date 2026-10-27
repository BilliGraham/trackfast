# Google pilot sign-in check

Google verifies identity only. Trackfast hosting and data storage remain separate.
The pilot uses an explicitly approved email. Google Workspace membership is not assumed.

`tools/google_signin.py` is an isolated registration check. It does not grant API access.
The API now has a separate Google session mode. See [API sessions](api-sessions.md).
The native app is not connected to this sign-in flow.

## Run the check

Create an isolated Python environment and install `tools/requirements-signin.txt` there.
Use Python 3.12 or later. On Windows, for example:

```powershell
python -m venv "$env:TEMP\trackfast-signin-venv"
& "$env:TEMP\trackfast-signin-venv\Scripts\python.exe" -m pip install -r tools/requirements-signin.txt
& "$env:TEMP\trackfast-signin-venv\Scripts\python.exe" -B tools/google_signin.py --client-id YOUR_DESKTOP_CLIENT_ID --email YOUR_APPROVED_EMAIL
```

Use the Desktop app client from Google Auth Platform.
If Google supplied a client secret, enter it at the hidden local prompt. Do not send it through chat.
The prompt shows no pasted characters. The tool confirms whether it received a nonempty value.
Desktop clients cannot keep that value confidential. PKCE protects the authorization-code exchange.
The browser requests only `openid email`. Choose the approved account and complete Google's sign-in steps.
The callback uses a temporary port on `127.0.0.1`. There is no public callback server.

The terminal reports success only after verification. The browser callback message alone is not a successful check.
The output includes the Google subject ID for a future explicit API membership binding.
No tokens, authorization responses, or secrets are printed or saved.
The tool makes no Trackfast, Xero, Gmail, Drive, or Calendar requests.

## Checks and limits

The tool generates a new state, nonce, and PKCE verifier for every attempt.
It checks the callback host, path, state, and unique authorization code.
Network calls use fixed Google HTTPS endpoints, bounded reads, timeouts, and no redirects or proxies.
Google's `google-auth` library verifies the signature, issuer, audience, and token times.
The tool also checks the nonce, authorized party, verified email, and nonempty subject ID.
It rejects every email except the one supplied for this local check.

This command-line email argument is a test input, not a server permission mechanism.
The later API must enforce its own trusted membership records and bind users to Google's issuer and subject ID.
Do not grant ongoing access using only an email suffix or a client-supplied approval list.
API sessions, renewal, revocation, and account disabling now have a separate implementation.
Secure client credential storage remains later work.

References: [Google desktop OAuth](https://developers.google.com/identity/protocols/oauth2/native-app)
and [Google ID-token verification](https://google-auth.readthedocs.io/en/latest/reference/google.oauth2.id_token.html).

## TDD

The first test run failed because the helper did not exist.
Tests use generated RSA keys and signed tokens. They do not contact Google.
They cover identity scopes, PKCE, callback validation, wrong issuer and audience, expired and future tokens,
nonce mismatch, wrong or unverified email, invalid signatures, and safe exchange errors.
Passing tests do not prove that the registered Google client can complete a live sign-in.
All six sign-in tests passed on Windows and Linux. All 29 Python checks passed on Windows.
The Rust code did not change, so Rust checks were not repeated. macOS was not tested.
The user completed a live Google check successfully on October 27, 2026.
Signature, issuer, audience, token times, nonce, and the approved email all passed.
The user also completed the separate API session check on Windows on October 27, 2026.
API sign-in, renewal with unchanged ownership, and sign-out all passed. See [API sessions](api-sessions.md).

The first live attempt returned an identity token, then failed verification. Its broad error did not identify the cause.
The helper now reports fixed diagnostic messages for verification failures without printing private values.
The diagnostic tests failed before this change and passed afterward. Verification checks remain enforced.
