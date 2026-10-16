# Toggl integration boundaries

The scaffold makes no network requests.
These references define later integration work:

- [Track time entries](https://engineering.toggl.com/docs/track/api/time_entries/)
- [Track authentication](https://engineering.toggl.com/docs/authentication/)
- [API limits](https://support.toggl.com/en-us/article/api-webhook-limits-85auc8/)
- [Multiple timers](https://community.toggl.com/t/track-multiple-tasks-simultaneously/5069)

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
Never store an API token in an ordinary state file.

## Account type

The planned integration targets Toggl Track.
Toggl 2.0 uses a different API and authentication scheme.
Confirm the account type before implementing authentication.
