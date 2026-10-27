# Trackfast delivery roadmap

This file describes the target product. See architecture.md for the current implementation.
It records the agreed direction from the architecture and phased plan, updated October 26, 2026.

Trackfast has a shared Rust API with Postgres and a polished native reference client.
Internal teams can build their own clients, including TypeScript clients, against the same API.
Develop the native app and API independently. Connect the app after both work.
Keep production integration credentials on the server. Keep local work when the network is unavailable.

| Phase | Deliverable |
| --- | --- |
| 1 | Shared entry rules, settings, permissions, OpenAPI, examples, and contract tests. |
| 2A | A working API with identity, persistence, entry operations, settings, and access tests. |
| 2B | A refined local app with both entry modes, rounding, weekly targets, and visual settings. |
| 3 | Connect the app to the API with durable offline operations and conflict handling. |
| 4 | Xero exports, entry states, restricted PM weekly totals, and donut charts. |
| 5 | A small pilot with checked totals, actionable failures, and gradual user expansion. |

Phase 1 supplied the draft contract. Phase 2A now has a local development API for completed timed and duration-only entries.
See server.md for supported operations, authentication limits, and Postgres checks.
At the user's request, phase 3 now has a separate durable queue and local HTTP transport in `sync-client/`.
Create, edit, and delete retries preserve their keys across queue restarts. Conflicts retain local work.
See sync-client.md for its scope and the remaining connection work.
Phase 2 API and app prerequisites remain incomplete. The queue does not connect the daily-use app yet.
Review the open decisions in api-contract.md before implementing dependent behaviour.
The user approved half-interval ties rounding up, visible zero totals without export, and setting changes for new entries only.
Personal settings and duration rounding now work in the API. New accounts default to five minutes, 35 weekly hours, and the progress indicator.
Existing entries retain their applied rounding interval. The native app now has independent local settings and both entry modes.
The local app also has saved drafts during writes, entry correction, weekly targets, and a progress bar.
Calendar drag selection and a separate Day blocks view now support direct entry creation.
The native glass indicator was removed after user review.
See local-refinement.md for the phase 2B changes and validation. The native app remains separate from the API.
Timed entries now use their start date in a saved time zone. Project membership checks and project pagination now work in the API.
The queue now supports timed entries and project assignments. Entry-list pagination, ordered remote changes, and project administration now work.
The queue retains remote conflicts and checkpoints across restarts. See retrieval-admin.md.
Scheduled retries and explicit conflict choices now exist in the separate sync client. See [sync recovery](sync-recovery.md).
The API and queue now preserve an optional recording-time rounding choice across offline delivery.
The first native connection will sync new work only. Existing history will remain local. New offline work will remain queued until confirmed.
Shared deployment and native connection work remain unfinished.
Calendar-based holiday and leave adjustments are later work. Keep the configurable base target separate from calculated weekly adjustments.
The pilot will use Google sign-in with an explicit approved-user list. Google will not host Trackfast data.
Workspace membership is not assumed. An isolated registration check is available in google-signin.md.
The local pilot API now supports Google identity, stored membership, expiring sessions, renewal, and revocation.
See api-sessions.md. The native app remains separate.
The live API session check passed on Windows on October 27: sign-in, renewal with unchanged ownership, and sign-out all passed.
The separate Rust client now supports secure refresh storage, renewal, restart recovery, and sign-out cleanup.
See client-sessions.md. The live Rust check passed on Windows on October 27.
It covered Google sign-in, credential restoration in a new client instance, renewal, and sign-out cleanup.
The native sign-in interface now supports restoration, browser sign-in, and sign-out. See [native account connection](native-account.md).
Native time-entry sync, shared hosting, and operational controls remain unfinished.
Start phase 2A with duration-only entry creation, retrieval, replacement, and deletion.
Do not connect the native app to HTTP during phase 1 or phase 2.

The user reported successful Xero demo read and write tests on October 26.
The write test covered creation, repeated creation without duplicates, correction, and cleanup.
The two listed users shared one ID. Distinct-user access remains untested.
The retry test simulated response loss at the caller. Durable restart recovery remains untested.
These results permit development. They do not prove production permissions or invoicing behaviour.
Verify permissions before enabling another person's export. Complete operational checks before shared rollout.

App refinement is part of each native-app task. Test layouts, keyboard use, feedback, and failure recovery.
Prioritize Windows and Linux. Keep macOS portability and report each platform's actual checks separately.
ClickUp writes, calendar planning, MCP assistance, and a separate TypeScript app remain later tasks.
