# TestIT Implementation Tracker & Delivery Progress

**Project:** Backend Automation Suite Builder (`TestIT`)  
**Design Pack Reference:** `design/` (v1.2, October 2026)  
**Primary Deployment Target:** Docker & Docker Compose (Containerized Control Plane, Isolated Workers, Private NATS Core, React UI)  
**Status:** Prototype in progress; design acceptance is not met. See [PROJECT_REVIEW.md](PROJECT_REVIEW.md) for code-backed findings.
**Last Updated:** 2026-10-07

---

## 1. Executive Summary & Architecture Blueprint

The repository contains a working prototype, but it does not yet meet the design pack's release or security acceptance criteria:
1. **Control Plane (`services/control-plane`)**: Rust/Axum and SQLite code provides asset, environment, and run APIs. Authentication/RBAC are stubbed; the current identity endpoint returns a default administrator.
2. **Execution Plane (`workers/python`)**: Python worker protocol and connector adapters exist, but the Rust orchestrator does not invoke them. The current node dispatcher can report success without executing a node.
3. **Real-time Bus (`deploy/nats`)**: NATS configuration and an outbox publisher exist. The SSE handler currently polls SQLite and ignores `Last-Event-ID`; reconnect guarantees are not implemented.
4. **Web Interface (`apps/web`)**: React + TypeScript + Vite app with an ordered-step editor, run view, environment forms, and import/preview dialogs. The app now targets desktop only. The former “canvas” is not a graph editor; graph edges and the React Flow interaction described in the design are not implemented.
5. **CI Automation (`apps/cli`)**: A CLI exists, but Compose end-to-end execution remains unchecked and worker execution is not wired through the control plane.

---

## 2. Design safety goals (acceptance is not yet verified)

| Area | Design safeguard | Current implementation status |
|---|---|---|
| **Data Integrity** | Transactional Outbox + SQLite WAL | Some outbox/snapshot code is present. Run creation and cancellation do not consistently write durable events/outbox state atomically. |
| **Recovery** | Bounded Leases & Interrupted Flag | Not implemented end to end. There is no live worker process to supervise, and cancellation does not stop orchestration. |
| **Worker Isolation** | Ephemeral Containers & Strict I/O | Worker image/adapters are present but not called by the orchestrator; the control plane mounts the Docker socket. |
| **Secrets & Security** | Masked Previews & Envelope Encryption | Encryption code exists and this change removes the hard-coded key fallback. Authentication, authorization, and key rotation are absent. |
| **Real-time Transport** | Ephemeral NATS with SQLite Reconnection | NATS publisher/outbox code exists; current SSE polls SQLite and does not resume from the client's event ID. |
| **Variables & Functions** | Sandboxed Evaluator | Resolver and preview code exist; publication/run-time validation coverage is not established by the current acceptance evidence. |

---

## 3. Implementation Phases & Status Matrix

| Phase | Description | Status | Progress |
|:---:|---|:---:|:---:|
| **Phase 1** | **Repository Foundation, Schemas & Docker Setup** | Completed | 100% |
| **Phase 2** | **Rust Control Plane (Axum, SQLx SQLite, Auth, Variable Resolver)** | Partial | 45% |
| **Phase 3** | **Python Worker Runtime & Connector Protocol** | Partial; not wired into orchestration | 50% |
| **Phase 4** | **NATS Core Bus, Transactional Outbox & SSE Gateway** | Partial | 40% |
| **Phase 5** | **Desktop Web UI, Ordered-Step Editor & Run Monitor** | Partial | 45% |
| **Phase 6** | **CI CLI, Portability & End-to-End Verification** | Partial; E2E unchecked | 35% |

Progress percentages are rough implementation estimates, not acceptance results. A checked inventory item means code or configuration is present; it does not claim end-to-end behavior unless the acceptance evidence is recorded.

---

## 4. Detailed Component Checklist & Milestones

### Phase 1: Foundation, Schemas & Docker Deployment Setup
- [x] Git repository initialization and `.gitignore` setup
- [x] Implementation tracker document (`IMPLEMENTATION_TRACKER.md`)
- [x] Project directory structure creation (`apps/`, `services/`, `workers/`, `schemas/`, `deploy/`, `migrations/`, `fixtures/`)
- [x] Pinned schemas:
  - [x] `schemas/workflow.schema.json` (Suite, Case, Node graph, inputs, retry policies)
  - [x] `schemas/worker-envelope.schema.json` (Input invocation, progress frame, result envelope)
  - [x] `schemas/variables/variable-catalog.schema.json` (Built-in functions, typed scopes)
  - [x] `schemas/api-spec.schema.json` (OpenAPI normalized structures)
- [x] Docker primary configuration:
  - [x] `docker-compose.yml` (NATS Core, Control Plane, Web UI, isolated worker network)
  - [x] `deploy/nats/nats-server.conf` (Private, no auth leaking, subject rules)
  - [x] `services/control-plane/Dockerfile` (Multi-stage Rust builder)
  - [x] `workers/python/Dockerfile` (Pinned Python 3.11 + Playwright + DB adapters)
  - [x] `apps/web/Dockerfile` (Node Vite build + NGINX serve)

### Phase 2: Rust Control Plane Service
- [x] Cargo workspace configuration (`Cargo.toml`)
- [x] SQLite database migrations (`migrations/0001_initial_schema.sql`):
  - [x] Workspace, User, Session, Role Grants
  - [x] Asset, AssetRevision, AssetDependency (Immutable versioning, cycle detection)
  - [x] Environment, ConnectionProfile, SecretRecord (AES-256-GCM encrypted storage)
  - [x] SuiteRun, CaseRun, StepRun, RunProgressSnapshot, RunEvent
  - [x] NatsEventOutbox, ResourceLock, ArtifactRecord, AuditEvent
- [x] Secret Management & AES-256-GCM encryption layer (`crypto.rs`)
- [x] Sandboxed Typed Variable Evaluator (`variables.rs`):
  - [x] Scopes: `sys.*`, `env.*`, `suite.*`, `case.*`, `iteration.*`, `step.*`
  - [x] Pure built-in functions: UUID, Seeded Random, String manipulation, Datetime arithmetic, JSONPath selection
  - [x] No-network Variable Preview API (`/api/v1/variables/preview`)
- [x] Asset Draft & Revision Publishing Engine (`api/assets.rs`):
  - [x] Kahn's graph cycle detection, dependency pinning, content SHA-256 hashing
- [~] Execution Orchestrator & Capacity Controller (`orchestrator/mod.rs`): run rows and a concurrency limit exist, but node execution is stubbed and can report success without invoking the worker.
  - [~] Host-wide concurrency gate (configured default is 4; runtime acceptance not verified)
  - [ ] Run state machine based on actual worker results and interruption recovery
  - [~] Outbox publisher loop exists; durable event creation/reconnect acceptance is incomplete

### Phase 3: Python Execution Worker Runtime (adapters exist; control-plane integration missing)
- [x] Python Worker Protocol CLI (`workers/python/main.py`):
  - [x] Stdin/file envelope parsing and schema validation
  - [x] Stdout NDJSON progress and terminal result emission
  - [x] Stderr log capture and crash isolation
- [~] Adapters (implemented in worker source; not dispatched by the Rust orchestrator):
  - [x] Playwright HTTP API Request (`api.request`) via `APIRequestContext`
  - [x] MySQL / MariaDB read-only query & assertions (`db.mysql`)
  - [x] Cassandra CQL read-only assertions (`db.cassandra`)
  - [x] MongoDB projection & filter assertions (`db.mongodb`)
  - [x] Parquet / Delta Lake read & count verification (`data.tabular`)
  - [x] Sandboxed script execution for approved scripts (`script.python`, `script.shell`)
  - [x] Condition / Polling ("Wait Until") engine

### Phase 4: NATS Core & SSE Event Gateway (partial)
- [x] Pinned NATS Core publisher in Rust (`async-nats`)
- [~] Outbox publisher loop with retry; end-to-end event durability/recovery is not verified
- [x] Axum SSE gateway (`/api/v1/runs/{id}/events`):
  - [x] Initial snapshot push from SQLite
  - [ ] `Last-Event-ID` resumption (the current stream starts polling at sequence 0)
  - [x] Heartbeat keeping alive connections
  - [x] Degradation fallback to bounded polling if NATS is temporarily unavailable

### Phase 5: Desktop Web UI Frontend
- [~] React + TypeScript + Vite UI; Tailwind styling/build configuration added in this review.
- [~] Ordered-step workflow editor (`WorkflowCanvas.tsx`); a React Flow graph/edge canvas is not implemented.
  - [~] Node catalog includes API Request, DB Check, Tabular Data, and Wait controls; worker execution coverage is incomplete.
  - [x] Node inspection and parameter configuration sidebar
- [~] Asset & case management (`SuitesView.tsx`): case drafts and publish action exist; suite composition, revision diffing, and publish review modal are missing.
- [~] Variable preview (`VariablePreviewModal.tsx`); variable picker integration is missing.
- [~] Live Run Monitoring View (`LiveRunMonitor.tsx`): UI subscribes to SSE, but event replay and actual worker-driven statuses are incomplete.
  - [~] Progress display and status transitions via SSE; current percentages/statuses are not fully backed by real execution snapshots.
  - [~] Step event timeline; sanitized-log acceptance is not verified.
  - [x] HTML, JUnit XML, and CSV export links are present.
- [~] Connections, Environments & Secrets screens exist; access control and environment-scoped secret flows are missing.
- [~] OpenAPI Import Wizard exists; API contract/import acceptance is incomplete.

### Phase 6: CI CLI Tool, Portability & Verification
- [x] CI Command-Line Client (`apps/cli`):
  - [x] Trigger run, wait for completion, stream status, download JUnit/HTML reports, return exit codes
- [x] Fixture Suite (`fixtures/sample_suite.json`)
- [ ] End-to-End Docker Compose launch and execution test
- [ ] Implement bundle import/export; no bundle engine is currently present.
- [ ] Implement authenticated identity/RBAC before treating any API route as workspace-isolated.
- [ ] Wire worker dispatch, timeout, cancellation, and result persistence before reporting run success.

---

## 5. Continuity Instructions for Future Sessions

When resuming development:
1. Review this document to verify the current phase and active task.
2. Confirm Docker daemon status via `docker ps`.
3. Check the Rust control plane (`cargo test`), Python worker test suite, and Frontend (`npm run build`).
4. Update the checkboxes and progress metrics in this document upon completing each sub-task.
