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

Phase 1 supplied the draft contract. Phase 2A now has a local development API for exact duration-only entries.
See server.md for supported operations, authentication limits, and Postgres checks.
Review the open decisions in api-contract.md before implementing dependent behaviour.
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
