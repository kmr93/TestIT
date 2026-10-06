# Architecture and Execution Design

## 1. System shape

The control plane and data plane are separated. The Rust service owns identity, authoring, versioning, scheduling, durable run state, reports, secrets, and worker lifecycle. Short-lived workers perform a single bounded node invocation.

    Desktop Browser UI (React + TypeScript + React Flow)
          | HTTPS REST + authenticated Server-Sent Events
          v
    Rust control plane (Axum + Tokio)
      |-- SQLite file: users, assets, revisions, runs, events, audit, outbox
      |-- variable resolver: typed scopes + versioned safe-function catalog
      |-- NATS Core publisher/subscriber + per-run authorized SSE gateway
      |-- local artifact store: bounded logs and report artifacts
      |-- secret manager: encrypted credential payloads + external master key
      |-- worker launcher: creates ephemeral node worker containers
      |-- mail adapter: SMTP after durable completion
          |
          +--> Python worker image (Playwright API, DB/data adapters, script runtime)
                  |--> approved API/database/storage endpoints only

    Private Docker Compose network: one NATS Core service; no host-published client port

The UI does not connect to target systems. It sends typed definitions to the API. The API validates and persists them. The executor obtains the immutable definition and the exact referenced secrets, then starts a worker with only the data required for that node.

## 2. Technology decisions

- Axum is the Rust HTTP framework; its documented design builds on Tokio/Tower and supports middleware such as tracing and timeouts. See [Axum documentation](https://docs.rs/axum/latest/axum/).
- SQLx manages the authoritative SQLite database and schema migrations. SQLx documents SQLite plus MySQL/MariaDB support; use SQLite only for platform metadata in v1. See [SQLx database support](https://docs.rs/sqlx/latest/sqlx/database/index.html).
- React Flow provides the canvas interaction for nodes and edges. Persist a platform-owned workflow schema, not the UI library's internal node object shape.
- Playwright Python APIRequestContext handles API nodes without browser launches. See [Playwright APIRequestContext](https://playwright.dev/python/docs/api/class-apirequestcontext).
- Use one private NATS Core service for live-stat fan-out. Core NATS is lightweight and ephemeral; SQLite remains the durable source and provides reconnect snapshots. NATS documents Core delivery as at-most-once and JetStream as the persistence/replay layer: [Core NATS](https://docs.nats.io/learn/core-nats/), [JetStream](https://docs.nats.io/reference/2.12/jetstream).
- Python worker adapters cover user Python/shell assets, Cassandra/MongoDB, MySQL/MariaDB, Parquet, and Delta Lake. The delta-rs project provides Rust and Python APIs and documents supported storage backends in its [feature table](https://delta-io.github.io/delta-rs/latest/feature-table/).
- Executor containers run with least privilege and resource limits. Docker documents rootless operation and resource/security options; see [rootless mode](https://docs.docker.com/engine/security/rootless/) and [container run security options](https://docs.docker.com/reference/cli/docker/container/run/).

## 3. Main components

### Web client

- Suite/case library, visual editor, environment and connection configuration, run detail/report view, import/export wizard.
- Generates typed REST requests from a versioned schema.
- Uses Server-Sent Events for run status and falls back to polling.
- Connects only to the Rust API; no NATS credentials or broker socket reach the browser.
- Provides a desktop canvas editor for supported browser viewports of at least 1280 × 720. Mobile and tablet layouts are not supported targets.
- Provides variable picker, formula/function picker, no-network variable preview, and run comparison.
- Never stores secret plaintext in browser local storage; secret entry fields clear after save.

### Rust control plane

- Authentication and role checks.
- Workflow and connector validation.
- Revision creation and dependency pinning.
- Run queue, capacity control, cancellation, worker heartbeat/deadline management.
- OpenAPI import/normalization, data-set iteration planning, environment locks, and live progress/stat aggregation.
- Versioned typed-variable resolution and safe-function evaluation shared by preview and execution.
- NATS Core publisher/outbox drain and authenticated per-run event gateway.
- Run comparison against the last successful run for the same suite revision/environment.
- SQLite transactions and migration control.
- Secret encryption/decryption boundary.
- Report generation, retention, audit, email outbox.
- Health/readiness and Prometheus-compatible metrics. Report NATS connection state as a live-channel health signal; a broker outage degrades real-time delivery but does not make durable storage or execution readiness unavailable when polling fallback remains functional.

### Python node worker

- Accepts a versioned invocation envelope on stdin or a private mounted input file.
- Emits bounded, versioned NDJSON frames on stdout: validated progress frames while running and exactly one final result frame. Bounded logs go to stderr. The coordinator rejects malformed frames and never treats a progress frame as a node result.
- Progress frames contain only phase, safe counters, and timing data. They cannot contain request/response bodies, secrets, or arbitrary user values. The coordinator coalesces progress to at most one update per second per run, while durable status transitions publish immediately.
- Executes one node then exits. A worker cannot fetch packages or change its runtime.
- Receives only declared inputs, connection reference material needed for that step, and its output directory.
- Adapter protocol is stable across worker image upgrades; each invocation records worker image digest and adapter version.

### NATS Core and event gateway

- Ship one pinned NATS server in the Compose profile on a private network; disable JetStream for v1 and publish no host port.
- Rust persists each run transition/stat snapshot and an event-outbox record in a SQLite transaction, then publishes a versioned compact snapshot to a run-scoped NATS subject.
- The Rust SSE gateway subscribes to the authorized run subject and forwards updates to clients after workspace/run authorization. It can also send the current SQLite snapshot at connection time.
- Sequence numbers make duplicate/out-of-order messages safe to ignore. If NATS is unavailable, the API serves snapshots and the UI switches to bounded polling with a stale/degraded indicator. The SQLite outbox retries publishing after recovery; because Core NATS is ephemeral, reconnecting clients always reconcile against a fresh SQLite snapshot rather than relying on delivery of every intermediate event.
- Keep NATS Core messages ephemeral. Use JetStream only if later requirements need broker-side replay/retention; SQLite already holds the durable run history.

### Storage

- SQLite is authoritative for metadata, definitions, run state, step results, audit, and email outbox.
- Large artifacts are kept in a local content-addressed directory with size caps and checksums.
- The database stores artifact metadata and paths, not arbitrary large blobs.
- The app and SQLite file must be on one local filesystem, not a network share.
- Provide a supported backup command and UI bundle exporter; do not copy the live database file blindly while WAL is active.

## 4. Execution lifecycle

1. Validate the requested suite revision, all dependency revisions, data set bounds, API specification references, and target environment.
2. Create a run row and immutable run manifest in one SQLite transaction. The manifest pins every suite, case, template, worker image, and connector version.
3. Enqueue the run and apply the host-wide active-case limit. Start with 4 active cases total.
4. Resolve the run's environment profile and required secret references. Record secret version IDs, never secret values.
5. For each case, evaluate its ordered graph. For each step, create a short-lived worker invocation with a unique idempotency key, a wall-clock deadline, resource limits, and step-scoped inputs/secrets.
6. Persist step start/result events. Cap and redact logs before storing; large outputs become artifact files.
7. Compute case and suite results only from durable step outcomes. A failed assertion is FAIL; worker loss/connector failure is ERROR; user stop is CANCELED; a step never started is SKIPPED.
8. After each durable state transition, update the live run snapshot and insert an event-outbox record in the same SQLite transaction. Publish versioned stats to NATS Core, then forward through authenticated SSE. Emit aggregate statistics at most once per second and status transitions immediately.
9. Write the final run status and an email outbox item atomically. A separate sender retries SMTP delivery with bounded backoff.
10. Apply retention to run artifacts and event payloads; preserve configuration revisions and audit records according to policy.

### Live progress and statistics

The run snapshot is derived from durable run/case/step records and sent from NATS Core through the authenticated SSE stream. It includes total and terminal case counts; case counts by PASS/FAIL/ERROR/CANCELED/INTERRUPTED; total and terminal node counts; active, queued, failed, skipped, and retry-wait nodes; elapsed time; current case and step; and last update time. Optional technical statistics include API request count, status-class counts, latency p50/p95, observed request rate, and bounded DB-query counts/latency. Never include raw request/response bodies, secret values, or unbounded labels.

The progress percentage is terminal node outcomes divided by the planned node count for the pinned run graph. Branches resolved as skipped count as terminal outcomes, so the percentage can advance when a branch is selected. Data-set iterations are expanded into planned case iterations before execution. For a dynamic path whose total work cannot be determined, show completed counts and “remaining work unknown” instead of inventing a precise percentage. An ETA is optional, explicitly labeled estimated, and shown only when there is enough comparable history for the same suite revision and environment.

The browser receives a current SQLite-backed snapshot and then NATS-backed state-change events over SSE. Sequence numbers support reconnect/deduplication; after a disconnect the UI requests a fresh snapshot and resubscribes. Heartbeats keep idle SSE connections alive. UI display refresh target is 2 seconds; worker logs remain separately bounded and can be throttled without delaying status updates.

### Recovery rules

External side effects cannot be made exactly-once by this platform. If the process dies after a remote system performed an action but before the result was recorded, the action's outcome is ambiguous. Mark that step INTERRUPTED/UNKNOWN and fail the run. Automatically replay only explicitly idempotent read-only or idempotency-key-protected actions. Never silently relabel the interrupted step as successful.

A restart recovers queued work and expired leases. A lease includes a worker ID, heartbeat, and deadline. The scheduler may requeue a node only if it is declared safe to replay. Otherwise it records an interrupted outcome and waits for a human-triggered rerun.

## 5. Concurrency and durability

- One host-wide scheduler is the sole run claimant.
- A serialized SQLite writer path batches run-event writes and keeps transactions short.
- Enable WAL and foreign keys; set busy timeout; use bounded connection pools.
- Start with 4 active test cases total and queue the rest. Default each case to sequential steps.
- Acquire configured environment/resource locks before starting mutating cases. Locks have owner, lease, heartbeat, and timeout; expired locks are surfaced for operator review before release when side effects may be in progress.
- Parallel data iterations only when the suite allows it and all declared shared resources are either isolated or protected by locks.
- The Rust variable resolver evaluates safe built-ins against a pinned function-catalog version. It resolves typed references before invoking workers; workers receive concrete typed values and secret handles only.
- Variable preview uses the same resolver with sample inputs and masked secret placeholders. It performs no network call and does not execute a worker.
- Run comparison is computed from the current run and the latest prior PASS with the same suite revision/environment. Compare outcome counts, case/step statuses, durations, and API latency summaries; omit payload and row-value comparisons.
- Keep run payloads/logs bounded. Store bulk output as files.
- Migrate to PostgreSQL and multi-host workers only behind a deliberate storage-interface change, not by placing SQLite on a shared network filesystem.

## 6. Connector strategy

Use a uniform Python worker adapter contract in v1, even where Rust drivers exist. This avoids separate node semantics and makes approved Python assets and Playwright API calls share one worker platform. SQLx and Rust libraries remain appropriate for the Rust control plane's own storage and any future high-volume typed connector.

| Target | v1 operation | Access |
|---|---|---|
| MySQL / MariaDB | parameterized read-only query, row count, selected-value extraction | connection profile; read-only account required |
| Cassandra | parameterized CQL select and selected-value extraction | cluster profile; least-privilege role |
| MongoDB | filter/projection/aggregation read checks and extraction | connection profile; read-only role |
| Parquet | schema, count, selected columns and predicate checks | local allow-listed path or S3-compatible URI |
| Delta Lake | version-pinned table read/schema/count checks | local or S3-compatible storage first |
| API | HTTP request and response assertions | Playwright Python APIRequestContext |

Connector support is not just a driver install: each adapter needs version support, TLS/auth configuration, timeout behavior, size limits, and integration tests against declared server versions.

## 7. Observability

- Structured JSON logs with run_id, case_id, step_id, worker_id, and correlation_id.
- Event stream for execution status; metric labels must not include secret values, raw URLs with tokens, or unbounded suite names.
- Live run stats: case/node status counters, elapsed time, active work, retry count, API request status/latency aggregates, and current step. Aggregate response metrics only; avoid exposing sensitive payload values.
- Platform metrics: queue depth, active workers, node duration/status counts, worker exits/timeouts, database busy events, report generation, SMTP failures, artifact bytes, and SSE subscriber count.
- NATS metrics: connected/disconnected state, publish retry depth, publish latency, and SSE subscriber fan-out; do not include run IDs or user values as unbounded metric labels.
- Health endpoint checks API and SQLite. Readiness also checks worker engine and writable storage.

