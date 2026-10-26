# Personal API settings

The API stores settings separately for each tenant and person.
The authenticated identity selects the owner. Requests cannot supply an owner or version in the body.
The native Settings screen is not connected to these endpoints yet.

| Field | Default | Accepted values |
| --- | --- | --- |
| `rounding_interval_minutes` | `5` | `5` or `null` for Off. |
| `weekly_target_hours` | `35` | A positive finite number. Fractional hours are allowed. |
| `visual_indicator` | `progress` | `progress` or `glass`. |

The three fields are independent. The weekly target does not cap, split, or change recorded time.

## Read and change

`GET /v1/me/settings` returns the fields, their version, and an ETag.
The first read creates the default settings if needed. A new entry also initializes missing settings.

`PUT /v1/me/settings` replaces all three fields. For example:

```json
{
  "rounding_interval_minutes": null,
  "weekly_target_hours": 32.5,
  "visual_indicator": "glass"
}
```

Send the latest ETag in `If-Match` and a new UUID in `Idempotency-Key`.
Keep that key, body, and ETag when retrying the same request.
An identical retry returns the stored result, even if its ETag is now old.
A different request with that key returns 409. A new request with a stale ETag returns 412.
Missing preconditions return 428. Invalid fields return 422.
Settings, their audit record, and the operation receipt commit in one transaction.
Settings and entry operations share the same operation-key namespace for the authenticated account.

## Rounding

Original seconds always remain unchanged. The API calculates a separate effective duration.
Five-minute rounding uses the nearest 300 seconds. An exact half interval rounds up.

| Original seconds | Effective seconds with five-minute rounding | Effective seconds with Off |
| --- | --- | --- |
| 149 | 0 | 149 |
| 150 | 300 | 150 |
| 780 | 900 | 780 |
| 781 | 900 | 781 |
| 10800 | 10800 | 10800 |

Zero effective durations remain visible in entry retrieval and lists.
They must not be exported when the export worker is implemented. This API does not send Xero exports.
Add effective durations directly for totals. Do not round the total again.

A new entry uses the server setting at its first successful creation.
Its response records the applied interval. A retry returns that original result after a settings change.
Changing settings does not update existing entries.
Editing an entry recalculates its effective duration with its original applied interval.
Entries from the earlier exact-only API retain rounding Off after migration.

Offline recording-time settings snapshots are not implemented. They require a separate contract decision before the native app connects.
The development client accepts and checks both exact and rounded API responses.
Remote reads expose the original duration, effective duration, and applied interval.

## Later calendar adjustments

The configurable weekly target is the base target.
A later feature can derive a weekly target from selected calendar holidays and scheduled leave.
Keep the base setting separate from those adjustments. Do not change recorded durations to adjust a target.
Calendar selection, working days, partial-day leave, and overlapping events need rules before that feature starts.
No calendar connection or automatic adjustment is included in this change.

## TDD evidence

The initial settings test failed with 404 before the endpoint existed.
The initial rounding test returned one effective second where zero was expected.
After implementation, both tests passed against Postgres.
The existing sync tests then rejected rounded responses. They passed after receipt validation supported both modes.
The contract test failed before the progress indicator default was documented.
Additional validation, concurrent update, rollback, tenant isolation, and migration checks passed without another implementation change.

Validation on October 26, 2026:

- Windows and Linux: 17 API tests, 14 sync tests, and both builds passed.
- Windows: formatting and Clippy checks passed for both crates.
- Windows: all 51 native-app tests, the native build, and all 21 Python checks passed.
- Linux checks used a disposable Rust 1.98 container and Postgres 17.
- macOS and visual checks did not run. The native interface did not change.
