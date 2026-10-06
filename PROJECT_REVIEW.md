# TestIT implementation review

**Reviewed:** 2026-10-07
**Design baseline:** `design/` v1.2, updated here for desktop-only product scope
**Conclusion:** The repository is a useful prototype, but it does not meet the design pack's execution, security, or end-to-end acceptance criteria. The implementation tracker previously overstated completion; it now points to this review and marks the affected phases partial.

## Findings

### P1 — Runs can report success without executing workflow nodes

`services/control-plane/src/orchestrator/mod.rs` dispatches node execution as a timed placeholder and returns `SUCCEEDED` for node types instead of invoking the Python worker. The Python worker and adapters exist, but the orchestrator does not launch them. As a result, a run can show PASS even when an API request, database check, script, or other node never ran. Worker timeout, result validation, and crash recovery are consequently not end-to-end behavior.

**Design impact:** Violates execution lifecycle, per-node adapter, failure-status, and CI exit-code acceptance requirements in `design/02-architecture.md` and `design/08-delivery-and-acceptance.md`.

### P1 — API authorization is stubbed

`services/control-plane/src/api/auth.rs` returns the first database user or a fabricated default administrator and a fixed permission list. The API router does not apply authentication or per-workspace authorization middleware. `services/control-plane/src/main.rs` configures permissive CORS. Run, environment, asset, and secret endpoints therefore do not meet the design's workspace isolation or role requirements.

**Design impact:** Violates the authentication/RBAC requirements in `design/02-architecture.md`, `design/05-api-contract.md`, and the Security acceptance section in `design/08-delivery-and-acceptance.md`.

### P1 — Control plane mounts the host Docker socket

`docker-compose.yml` mounts `/var/run/docker.sock` into the control plane. Access to this socket can grant broad control over the host Docker daemon. The design calls for tightly bounded isolated workers and a threat model; the current deployment does not establish a safe boundary around this host-level capability.

**Design impact:** Violates the worker isolation/security acceptance criteria in `design/02-architecture.md` and `design/08-delivery-and-acceptance.md`.

### P1 — Cancellation only changes the database row

`services/control-plane/src/api/runs.rs` marks a queued or running run `CANCELED`, but the orchestrator does not observe a cancellation signal or stop active work. It can continue processing and later write a different terminal state. Cancellation also does not consistently update the event/outbox records in the same transaction.

**Design impact:** Violates execution recovery and cancellation acceptance criteria in `design/02-architecture.md` and `design/08-delivery-and-acceptance.md`.

### P2 — Event replay does not honor reconnect position

`services/control-plane/src/api/events.rs` starts polling from sequence `0` for every connection and does not read the request's `Last-Event-ID`. Reconnecting clients can receive old events again; the promised no-gap/no-duplicate resume behavior is not implemented. The handler currently polls SQLite rather than forwarding live NATS updates.

**Design impact:** Partially implements the event stream, but does not meet reconnect, sequence, or NATS gateway requirements in `design/02-architecture.md` and `design/08-delivery-and-acceptance.md`.

### P2 — The workflow editor is a sequence list, not the designed graph canvas

`apps/web/src/components/WorkflowCanvas.tsx` renders an ordered vertical list. It does not use React Flow, display graph edges, or provide branch/connection editing. `SuitesView.tsx` persists `edges: []`. The editor's node toolbar also includes types whose execution path is not connected to the worker.

**Design impact:** The UI does not match the visual node/edge canvas and graph authoring described in `design/00-README.md`, `design/02-architecture.md`, and `design/06-ui-and-user-flows.md`. This remains a known feature gap; the desktop-only change does not claim graph authoring is complete.

### P2 — The authoring screen cannot create a runnable suite

`SuitesView.tsx` creates assets with `kind: 'case'`, while the run endpoint accepts a `suite_revision_id`. The screen has no suite-composition flow. The UI now disables “Run Suite” for case assets and asks the user to pick an environment explicitly, preventing the previous case-revision/placeholder-environment request. A suite builder and real execution path are still required to complete this workflow.

### P2 — Portability bundles and several platform capabilities are absent

The web UI includes OpenAPI import, but the repository does not provide the design's validated project bundle import/export flow. Schedules/webhooks, email delivery, backup/restore and retention workflows, metrics, and parts of data-driven/setup/cleanup execution are also missing or unconnected. Existing schemas and tables are not proof of working user flows.

**Design impact:** Multiple requirements in `design/03-workflows-and-nodes.md`, `design/04-data-and-portability.md`, `design/07-security-reporting-operations.md`, and `design/08-delivery-and-acceptance.md` remain open.

### P2 — The CI client can wait forever

`apps/cli/src/main.rs` polls the run endpoint in an unbounded loop and does not apply a request timeout or overall run deadline. If the run never reaches a terminal state, or the service keeps returning non-success responses, the CI job can hang indefinitely.

**Design impact:** The CLI exists, but reliable bounded CI completion is not established as required by `design/08-delivery-and-acceptance.md`.

### Resolved in this change — The default secret key was hard-coded

The fixed `MASTER_KEY_HEX` fallback is removed from the Rust configuration and Compose now refuses to start unless a 64-character key is supplied. The README explains the key requirement and recovery risk. Key rotation and supported secret-manager integration remain open.

### Resolved in this change — The frontend utility classes were not being generated

The React screens used Tailwind utility classes, but the web package had no Tailwind/PostCSS dependency or configuration. The browser rendered the layout largely unstyled. Tailwind build configuration and dependencies are now present, the main stylesheet defines the shared desktop theme and controls, and the Google Fonts runtime dependency has been removed.

The mobile list editor and its switch have also been removed. The product requirements and UI/deployment acceptance documents now declare desktop-only support with a minimum viewport of 1280 × 720. npm audit initially reported nine issues in the old Tailwind/Vite toolchain; the project now uses Tailwind 4 and Vite 8, and the install audit reported zero vulnerabilities. The Docker web build now uses Node 24 and `npm ci`.

Other desktop usability changes include a first-use empty state, a named create-case dialog instead of a browser prompt, disabled actions when no valid selection exists, explicit environment selection, keyboard-operable asset selection, and in-page errors instead of browser alerts. The live run monitor now marks progress unavailable instead of inventing a percentage when the server cannot provide one. `Cargo.lock` is now tracked for reproducible Rust application builds.

## Design coverage snapshot

| Design area | Status | Review summary |
|---|---|---|
| Desktop shell and design system | Partial; visual baseline corrected | Utility CSS now builds and desktop minimum is documented. Screen-level loading, permission, validation, and error states still need work. |
| Workflow authoring | Partial | Ordered sequence editing exists; graph edges, branch authoring, many node configurations, and validated graph publication are incomplete. |
| Worker execution | Missing from orchestration | Adapters exist in Python but are not invoked by Rust. Current placeholder dispatch can report success without running a node. |
| Identity and access control | Missing | Default administrator identity and fixed permissions; no route-level authentication/RBAC. |
| Variables and secret handling | Partial | Resolver/preview and encryption code exist; the unsafe key fallback is removed, but key rotation and end-to-end authorization/redaction are not established. |
| Live run view and event replay | Partial | UI subscribes to SSE and API provides a stream, but resume ID and live NATS gateway behavior are incomplete. |
| Reports and comparison | Partial | Export/comparison endpoints and UI exist; report acceptance and security are not fully verified. |
| OpenAPI import | Partial | Validate/import UI exists; it does not fulfill the separate portable bundle import/export requirement. |
| Deployment and operations | Partial or missing | Compose exists and now requires a key, but Docker socket exposure, backup/restore, retention, and metrics remain unresolved. |
| End-to-end acceptance | Not met | Compose launch and real worker execution are unchecked. |

## Verification evidence

- Frontend production build: **passed** (`npm run build`) with Tailwind 4 and Vite 8.
- npm dependency audit after the toolchain update: **0 vulnerabilities**.
- Rust workspace compile: **passed** (`cargo check --workspace --locked`), with existing unused/dead-code warnings.
- Formatting: the changed Rust configuration file is formatted. `cargo fmt --all -- --check` still reports formatting differences across other existing Rust files; those unrelated files were not reformatted as part of this review.
- Earlier repository checks recorded in the prior review: control-plane check/tests and Python bytecode compilation passed; full workspace test harness exited with Windows `STATUS_ACCESS_VIOLATION` in the CLI test process.
- Docker Compose end-to-end launch and execution: **not run**; the tracker still records this as unchecked.

The build proves the frontend compiles and Tailwind emits CSS. It does not prove browser flows, API authorization, worker dispatch, deployment safety, or the design pack's acceptance criteria.
