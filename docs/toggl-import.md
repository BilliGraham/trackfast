# Toggl import rules

Trackfast can copy history from Toggl Track once, on request. Toggl is not a sync target.
These references define the import:

- [Track time entries](https://engineering.toggl.com/docs/track/api/time_entries/)
- [Track authentication](https://engineering.toggl.com/docs/authentication/)
- [API limits](https://support.toggl.com/en-us/article/api-webhook-limits-85auc8/)

## Requests

The import sends read-only requests to `https://api.track.toggl.com/api/v9`:

1. `GET /me?with_related_data=true` loads the clients and projects.
2. `GET /me/time_entries` loads the entries, one request for each 30 days from the start date.

Requests use HTTP basic authentication. The user name is the API token and the password is `api_token`.
The client requires HTTPS, rejects redirects, and has a 20-second timeout for each request.
HTTP 401 and 403 mean that Toggl did not accept the token.
HTTP 402 and 429 mean that the Toggl request quota is used up. The import stops and does not retry.
Free Toggl plans allow 30 requests per hour. The import counts its own requests and stops before that limit.
Toggl may not return entries older than a certain age through this endpoint. Toggl's error then appears in Settings.

## Token

The token lives only in memory for one import. Trackfast never writes it to disk or to a credential store.
The token field does not keep undo history. The app clears the field and its edit state when the import starts.
Never log the token or authorization responses.

## Mapping

- Each Toggl client, project, and entry keeps its Toggl ID in a `toggl_id` column with a unique index.
- A second import inserts only rows with new Toggl IDs. It does not change rows that an earlier import added.
- A Toggl client with the same name as a local client joins that local client.
- Archived Toggl projects become archived local projects.
- A project without a valid `#rrggbb` colour gets a grey colour.
- A negative duration identifies a running entry. The import skips running entries.
- A completed entry without a stop time gets a stop time from its start and duration.
- The database worker writes the whole import in one transaction.

## Not imported

Tags, tasks, billable flags, workspaces, and other users' entries are not imported.
