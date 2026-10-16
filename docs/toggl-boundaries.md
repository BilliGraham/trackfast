# Toggl integration boundaries

Trackfast reads Toggl Track API v9 data. It does not write to Toggl.
These references define the integration:

- [Track time entries](https://engineering.toggl.com/docs/track/api/time_entries/)
- [Track authentication](https://engineering.toggl.com/docs/authentication/)
- [API limits](https://support.toggl.com/en-us/article/api-webhook-limits-85auc8/)
- [Multiple timers](https://community.toggl.com/t/track-multiple-tasks-simultaneously/5069)

## Current requests

Each sync sends two requests to `https://api.track.toggl.com/api/v9`:

1. `GET /me?with_related_data=true` loads the user, workspaces, and projects.
2. `GET /me/time_entries` loads entries from local midnight at the start of last week until 24 hours from now.

Requests use HTTP basic authentication. The user name is the API token and the password is `api_token`.
HTTP 401 and 403 mean that Toggl did not accept the token.
The worker removes the rejected client and saved token, then returns the interface to sign-in.
If token removal fails, the interface shows the error and remains signed out.
HTTP 402 and 429 mean that the request quota is used up.
Trackfast blocks all sync attempts for one hour and does not retry automatically.
Startup, sign-in, and refresh share one worker-owned budget of 30 request attempts in a rolling hour.
The worker enforces a four-minute pause between sync attempts. Sign-out does not reset the budget.
Each HTTP request consumes budget before transmission, including requests that fail.
The budget belongs to the current process. It does not account for other apps or survive a restart.
The client requires HTTPS and rejects redirects.
Each request has a 20-second timeout.

The token field does not retain undo history. Submission and sign-out also remove its stored edit state.
A negative duration identifies a running entry. A completed duration-only entry can have a null stop time.

## Parallel tasks

Toggl Track supports one running timer per user.
The planned widget must keep independent timers locally.
When a task stops, it can submit a completed entry with explicit timestamps.
Verify overlapping completed entries against a test account before enabling sync.
Check workspace rules for entry creation and editing.

## Projects

Projects need names and colours. Entries need stable project identifiers.
Decide how project changes map to Toggl before implementing project writes.
Do not show a local-only change as a confirmed server change.

## Reliability

Persist active intervals and pending writes before claiming durable success.
Handle restart, sleep, clock changes, and widget closure without losing time.
Prevent duplicate entries after request timeouts or a process crash.
Respect API quotas. Do not poll once per second.
Never store an API token in an ordinary state file. Use the credential store.

## Account type

The planned integration targets Toggl Track.
Toggl 2.0 uses a different API and authentication scheme.
Confirm the account type before implementing authentication.
