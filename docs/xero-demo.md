# Xero demo connection check

This separate development tool checks OAuth and read-only Projects access.
It does not connect the desktop app to Xero or change time entries.
Python 3.12 or later is required. No additional Python packages are required.

1. Register a Xero Web app with redirect URI `http://localhost:8787/xero/callback`.
2. Open its Configuration page. Copy the client ID.
3. Generate a client secret. Keep the secret out of chat, screenshots, and files.
4. Run `python tools/xero_demo.py` from the repository root in a local terminal.
5. Enter the client ID. Enter the client secret at the hidden prompt.
6. Select only Demo Company (Global) in the browser. Approve the read-only Projects scope.
7. Read the result in the terminal. Clear the clipboard after credential entry.

The listener binds only to IPv4 loopback. Authorization expires after five minutes.
The tool checks OAuth state and rejects duplicate callback fields.
It disables HTTP redirects and proxy use for API requests.
Credentials and tokens remain in process memory. The tool does not request a refresh token.
The demo-name check prevents accidental selection of ordinary company connections.
It is not proof that a renamed organisation is a demo. Select the actual demo during authorization.

Closing the tool ends local token use. It does not revoke the app connection in Xero.
## Write test

Run `python -B tools/xero_demo.py --write-test` in your local terminal.
Enter the credentials again. Reuse the existing client secret if available.
Authorize Projects write access for Demo Company (Global) only.
Select your Projects user from the numbered list. Select 0 if your account is absent.

The tool requires exactly one New Website project and one Development task.
It creates a labelled 15-minute entry, reads it, repeats its create request, and checks for duplicates.
It changes the duration to 20 minutes and checks the saved values.
It deletes only entries with the exact unique test label and matching task and user IDs.
It checks the complete time-entry list to confirm removal.
If a second Projects user exists, it repeats the test with that user through the same connection.

The repeated create request uses the same idempotency key and payload.
The tool discards the first response before replay, but retains its ID for comparison.
This simulates response loss at the caller. It does not test an actual network interruption or recovery after a process restart.
The tool attempts cleanup after request failures. Network failures or process termination can prevent cleanup.
If cleanup fails, remove entries with the printed exact label manually from the demo project.
Do not remove unrelated entries. Check cleanup before repeating the test.

The user selection is manual. A two-user result proves access to those IDs in that demo connection only.
Production permissions, invoiced entries, and temporary locks still require separate tests.
No live write test result is claimed until the operator runs this command and reports the output.

Run the local guard tests with `python -m unittest discover -s tools -p test_xero_demo.py`.

References:

- https://developer.xero.com/documentation/guides/oauth2/overview
- https://developer.xero.com/documentation/guides/oauth2/tenants/
- https://github.com/XeroAPI/Xero-OpenAPI/blob/master/xero-projects.yaml
