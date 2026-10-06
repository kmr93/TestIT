# UI and User Flows

## 1. Navigation

Primary navigation:

- Suites
- Reusable assets
- Runs and reports
- Connections and environments
- Schedules (later release)
- Administration

Use a left navigation rail and a central work area. The suite/case canvas is the main authoring surface; forms, validation, and logs live in side panels rather than requiring users to write definition JSON.

## 2. Suite authoring

1. Select New suite and provide name, description, labels, and default environment.
2. Add existing published cases from the reusable asset picker or create a draft case.
3. Reorder cases; set per-case execution mode (sequential by default, parallel only for independent cases).
4. Set suite-level inputs and notification preference.
5. Review dependency versions and any update notice.
6. Validate and publish a suite revision.
7. Run now or copy the authenticated trigger API details.

The dependency panel must clearly distinguish pinned versions from newer available revisions. Updating a reusable case does not mutate a published suite.

## 3. Case visual editor

Recommended layout:

    ┌──────────────────────────────────────────────────────────────────────┐
    │ Case name · Draft v12 · Validate · Publish · Run                     │
    ├─────────────┬──────────────────────────────────┬─────────────────────┤
    │ Node catalog│ Canvas: ordered nodes/branches   │ Properties / inputs │
    │ API         │ [Request] → [Assert] → [DB check] │ Typed form fields   │
    │ DB          │                 └→ [Cleanup]      │ Variable picker     │
    │ Script      │                                  │ Secret references   │
    │ Flow        │                                  │ Timeout/retry       │
    ├─────────────┴──────────────────────────────────┴─────────────────────┤
    │ Validation results, dependency warnings, and run preview              │
    └──────────────────────────────────────────────────────────────────────┘

Interaction rules:

- Drag a node from the catalog; the app creates a node with required fields marked.
- Selecting a node opens a typed form with inline help and a variable picker.
- Connectors show allowed input/output types and reject invalid graph edges.
- “Test connection” runs a read-only check with strict timeout and no saved test output containing secrets.
- Save draft is distinct from Publish revision.
- A run preview lists the exact revision and target environment before execution.
- Advanced script assets are selected from an approved catalog; ordinary users do not paste arbitrary code into the canvas.
- OpenAPI import previews operations and schemas before creating editable request templates; remote references are fetched only from administrator-approved origins.
- Data-set configuration shows row count, total iteration cap, and how inputs map to case parameters.
- “Wait until condition” exposes poll interval, deadline, and a preview of the last observed result.
- Variable manager defines typed custom values or chooses a safe built-in function; variable picker inserts references into supported API fields.
- “Preview request” resolves variables using sample values, shows the generated request with secrets masked, and explicitly confirms no network call occurred.

## 4. Run and report view

Header: suite/revision, run ID, environment, initiated by, start/end time, final status, live connection indicator.

Main report:

- A live progress strip with exact completed/total case iterations and nodes, percent when the denominator is known, and elapsed time. ETA is labeled as an estimate and appears only with enough comparable history.
- Live counters for pass/fail/error/cancel/interrupted cases, active/queued work, skipped nodes, and retries.
- A selected case/step panel that shows the current action, elapsed time, wait-poll attempts, and sanitized recent events.
- Live API statistics when API nodes run: request count, status-class counts, and p50/p95 latency for the current run. Avoid presenting these as a load-test result.
- Expandable case tree; each step shows attempt, duration, status, sanitized error, assertions, and bounded output. Data-driven cases are grouped by iteration and input-row identifier.
- Live status for active runs; cancel action for authorized users; rerun failed cases as a new run with the same pinned suite revision.
- Download HTML, JUnit XML, or CSV.
- Copy run link and re-run pinned suite revision with editable input values.
- “Create new run from this revision” is distinct from replaying an interrupted action automatically.

### Live dashboard layout

    Suite: Payments regression · RUNNING · Updated 1s ago
    Progress: 42 / 100 nodes complete (42%) · 2 / 8 case iterations complete
    Cases: 2 passed · 0 failed · 1 running · 5 queued
    Nodes: 38 passed · 2 running · 1 retry wait · 55 queued · 4 skipped
    Runtime: 3m 04s · Active workers: 3 / 4 · Retries: 1
    API: 18 requests · 16 2xx · 1 4xx · 1 5xx · p50 82 ms · p95 241 ms · observed 5.9 req/min
    Current: Create customer → Wait for record (attempt 3)

The screen updates these values from server snapshots, not browser-side guesses. For conditional branches, skipped nodes count as terminal outcomes; if the denominator cannot be known, replace the percentage with exact counts and “progress estimate unavailable.” Display ETA only as “estimated” when enough comparable runs exist. Let users expand a case iteration to see its step timeline and sanitized events.

Email report link requires a valid authenticated session. Do not use public bearer links by default.

The live view subscribes to the durable event stream and reconnects after network loss. It shows the last update time and a stale-data indicator when disconnected. Users can switch between overview, case timeline, and statistics without losing the selected run. Statistics update at least every 2 seconds while the run is active; log text may be throttled independently.

### Run comparison

Completed-run reports offer “Compare with last successful run” for the same suite revision and environment. Show case/iteration status changes, step duration changes, and API latency deltas. Clearly label the baseline run and date. When there is no qualifying baseline, explain that state instead of showing an empty comparison. Do not diff raw request/response bodies or data-row values.

## 5. Connections and credentials

Create a connection profile by choosing a type and environment. Use separate credential fields and secret references. After save, render masked value and “replace secret”; never show the stored value again. Support a least-privilege connectivity test and show only sanitized diagnostics.

Connection configuration may be reused by several suites, but access is granted by workspace/environment role. Deleting a connection used by a published suite is blocked until the references are removed or migrated.

## 6. Import/export flow

Export wizard:
- choose definitions only by default;
- optionally include approved scripts;
- admin may select run history/artifacts explicitly;
- show a warning that credentials and signing secrets are never exported;
- generate a bundle and display checksum.

Import wizard:
- upload;
- show bundle identity, format version, asset list, size, conflicts, unresolved connections, and script review state;
- choose merge as new or admin-only replace;
- confirm and show results;
- on error, preserve rollback point and present actionable diagnostics.

## 7. Accessibility and usability

### Desktop layout and design system

- The supported browser viewport is at least 1280 × 720. Mobile and tablet support are out of scope.
- Use a three-pane editor with node catalog, canvas, and properties panel. Preserve a usable desktop workspace when panels or run details open.
- Run dashboards, reports, import/export, connection forms, and admin actions use desktop layouts; wide data tables may scroll within their panel.
- Use consistent typography, spacing, semantic color tokens, and visible text labels for statuses. Preserve contrast in light/dark modes if both are offered.
- Preserve keyboard operations and visible focus on every desktop workflow. Do not make any authoring action available only through pointer drag-and-drop.

- Keep the visual system consistent: one spacing scale, form controls, buttons, status badges, alert patterns, loading skeletons, empty states, and destructive-action confirmation.
- Provide clear loading, validation, stale-data, offline, permission-denied, and recoverable-error states. Never make color the only signal for pass/fail/error.

- Keyboard-accessible canvas alternatives: node list, move up/down, connect via menus.
- Full keyboard navigation, visible focus, labels for status icons, color plus text for all statuses.
- Avoid relying only on drag-and-drop.
- Provide empty states with a sample read-only API suite.
- Show validation errors at the field and in one summary list.
- Confirm destructive actions such as cancel run, archive asset, or replace import.

