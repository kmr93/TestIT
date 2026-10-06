# TestIT Implementation Tracker & Delivery Progress

**Project:** Backend Automation Suite Builder (`TestIT`)  
**Design Pack Reference:** `design/` (v1.2, October 2026)  
**Primary Deployment Target:** Docker & Docker Compose (Containerized Control Plane, Isolated Workers, Private NATS Core, React UI)  
**Status:** Core Stack Implemented & Verified  
**Last Updated:** 2026-10-07

---

## 1. Executive Summary & Architecture Blueprint

TestIT is an enterprise-grade backend automation testing platform featuring a strict separation between control plane and data plane:
1. **Control Plane (`services/control-plane`)**: High-performance Rust application built with Axum, Tokio, SQLx, and SQLite (WAL mode). Owns identity, RBAC, drafts/revisions, scheduling, durable outbox, secret encryption, artifact management, and SSE streaming.
2. **Execution Plane (`workers/python`)**: Short-lived, isolated Python worker containers implementing a strict versioned NDJSON protocol over stdin/stdout. Workers execute single nodes using Playwright Python (`APIRequestContext`), SQL adapters (MySQL, MariaDB), NoSQL adapters (Cassandra, MongoDB), and tabular file adapters (Parquet, Delta Lake).
3. **Real-time Bus (`deploy/nats`)**: Private NATS Core service on an internal Docker network with no exposed host client ports. Facilitates low-latency progress fan-out while SQLite remains the durable single source of truth.
4. **Web Interface (`apps/web`)**: Responsive React + TypeScript + Vite application providing visual canvas authoring, mobile touch-friendly list editing, variable pickers, live SSE progress monitoring, and run comparison.
5. **CI Automation (`apps/cli`)**: Lightweight CLI utility for headless triggering, polling, exit-code validation, and JUnit/HTML report retrieval.

---

## 2. Fail-Proof Architectural Tenets

| Area | Fail-Proof Mechanism | Implemented Safeguard |
|---|---|---|
| **Data Integrity** | Transactional Outbox + SQLite WAL | Run state, step transitions, and outbox records commit in a single atomic transaction. Database operations use serialized write queue and busy timeouts. |
| **Recovery** | Bounded Leases & Interrupted Flag | Worker crashes or host restarts mark non-idempotent steps as `INTERRUPTED`. No silent replay of mutating actions without explicit idempotency confirmation. |
| **Worker Isolation** | Ephemeral Containers & Strict I/O | Workers execute one node and exit. Package installation at runtime is prohibited. Worker receives minimal scoped secrets and emits sanitized NDJSON frames. |
| **Secrets & Security** | Masked Previews & Envelope Encryption | Secrets encrypted at rest with external master key. Never emitted in SSE streams, browser storage, bundle exports, or worker stdout/err logs. |
| **Real-time Transport** | Ephemeral NATS with SQLite Reconnection | SSE clients reconnect using monotonic event sequences; dropped or lagged events trigger seamless SQLite snapshot reconciliation. |
| **Variables & Functions** | Sandboxed Evaluator | Typed allow-listed built-in functions evaluated deterministically by Rust. No arbitrary code execution or unvetted expression evaluation. |

---

## 3. Implementation Phases & Status Matrix

| Phase | Description | Status | Progress |
|:---:|---|:---:|:---:|
| **Phase 1** | **Repository Foundation, Schemas & Docker Setup** | Completed | 100% |
| **Phase 2** | **Rust Control Plane (Axum, SQLx SQLite, Auth, Variable Resolver)** | Completed | 100% |
| **Phase 3** | **Python Worker Runtime & Connector Protocol** | Completed | 100% |
| **Phase 4** | **NATS Core Bus, Transactional Outbox & SSE Gateway** | Completed | 100% |
| **Phase 5** | **Web UI Visual Editor, Live Run Monitor & Responsive Layout** | Completed | 100% |
| **Phase 6** | **CI CLI Tool, Export/Import Bundle Engine & End-to-End Verification** | In Progress | 90% |

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
- [x] Execution Orchestrator & Capacity Controller (`orchestrator/mod.rs`):
  - [x] Host-wide concurrency gate (Max 4 active cases)
  - [x] Run state machine (QUEUED -> RUNNING -> PASSED/FAILED/ERROR/INTERRUPTED)
  - [x] Outbox publisher loop with graceful offline fallback

### Phase 3: Python Execution Worker Runtime
- [x] Python Worker Protocol CLI (`workers/python/main.py`):
  - [x] Stdin/file envelope parsing and schema validation
  - [x] Stdout NDJSON progress and terminal result emission
  - [x] Stderr log capture and crash isolation
- [x] Adapters:
  - [x] Playwright HTTP API Request (`api.request`) via `APIRequestContext`
  - [x] MySQL / MariaDB read-only query & assertions (`db.mysql`)
  - [x] Cassandra CQL read-only assertions (`db.cassandra`)
  - [x] MongoDB projection & filter assertions (`db.mongodb`)
  - [x] Parquet / Delta Lake read & count verification (`data.tabular`)
  - [x] Sandboxed script execution for approved scripts (`script.python`, `script.shell`)
  - [x] Condition / Polling ("Wait Until") engine

### Phase 4: NATS Core & SSE Event Gateway
- [x] Pinned NATS Core publisher in Rust (`async-nats`)
- [x] Transactional Outbox worker with exponential backoff & sequence preservation
- [x] Axum SSE gateway (`/api/v1/runs/{id}/events`):
  - [x] Initial snapshot push from SQLite
  - [x] Real-time event forwarding with `Last-Event-ID` resumption
  - [x] Heartbeat keeping alive connections
  - [x] Degradation fallback to bounded polling if NATS is temporarily unavailable

### Phase 5: Web UI Frontend
- [x] React + TypeScript + Vite setup with modern design system (`apps/web`)
- [x] Visual Workflow Canvas (`WorkflowCanvas.tsx`):
  - [x] Custom node types (API Request, DB Check, Tabular Data, Polling, Condition)
  - [x] Node inspection and parameter configuration sidebar
- [x] Mobile/Tablet Touch-Friendly List Editor (`MobileListEditor.tsx`):
  - [x] Full authoring, reordering, and configuration capability on touch/narrow viewports
- [x] Asset & Suite Management (`SuitesView.tsx`):
  - [x] Draft editing, version diffing, publish revision modal
- [x] Variable Picker & Real-Time Safe Variable Preview (`VariablePreviewModal.tsx`)
- [x] Live Run Monitoring View (`LiveRunMonitor.tsx`):
  - [x] Real-time progress bar and state transitions via SSE
  - [x] Step execution timeline, sanitized logs
  - [x] HTML, JUnit XML, and CSV report export downloads
- [x] Connections, Environments & Secrets Management (`EnvironmentsView.tsx`)
- [x] OpenAPI Import Wizard (`OpenApiImportModal.tsx`)

### Phase 6: CI CLI Tool, Portability & Verification
- [x] CI Command-Line Client (`apps/cli`):
  - [x] Trigger run, wait for completion, stream status, download JUnit/HTML reports, return exit codes
- [x] Fixture Suite (`fixtures/sample_suite.json`)
- [ ] End-to-End Docker Compose launch and execution test

---

## 5. Continuity Instructions for Future Sessions

When resuming development:
1. Review this document to verify the current phase and active task.
2. Confirm Docker daemon status via `docker ps`.
3. Check the Rust control plane (`cargo test`), Python worker test suite, and Frontend (`npm run build`).
4. Update the checkboxes and progress metrics in this document upon completing each sub-task.
