# Product Requirements

## 1. Product objective

Give backend testers a visual, reusable way to compose, run, inspect, and share automation suites without writing a test harness. A suite contains test cases; each case contains ordered steps such as HTTP requests, assertions, script assets, database checks, extracts, sleeps, and calls to reusable cases.

The platform itself is Rust-first. It does not test web page interfaces in v1.

## 2. Users and permissions

- Admin: manages users, roles, environments, connections, secret access, worker images, mail, retention, and imports.
- Author: creates and revises suites, cases, reusable node templates, and script assets.
- Runner: starts/cancels runs and views permitted reports.
- Viewer: reads suites and reports without changing or triggering them.

Permissions apply at workspace and environment levels. A run uses the permission of the initiating user and records that identity.

## 3. Product principles

1. Visual forms and pickers are the normal authoring path.
2. Definitions are immutable once published; editing creates a new revision.
3. Every run captures the exact suite/case/node versions it executed.
4. Secrets are referenced by ID and injected only for the step that needs them.
5. Test execution is isolated and bounded. Scripts never run on the host.
6. A failed or interrupted external action is never reported as passed.
7. Configuration data, run state, logs, and large artifacts have explicit retention and export rules.

## 4. Scope

### v1 includes

- Visual suite and test-case editor with validation before save/run.
- API request node with method, URL/connection, headers, query, body, auth reference, timeout, and response assertions.
- Script library with immutable shell/Python assets. Authors select an approved version and fill typed inputs; inline code editing is out of scope for the no-code path.
- MySQL/MariaDB, Cassandra, and MongoDB validation/extract adapters.
- Read-only Parquet and Delta Lake checks; local filesystem and S3-compatible object access.
- Import of validated OpenAPI descriptions to create request templates and response-schema assertions.
- Common API authentication profiles: bearer token, API key, basic auth, and OAuth 2.0 client credentials with token refresh.
- Built-in variable/function picker and typed custom variables usable in API paths, query values, headers, bodies, script inputs, and assertions.
- No-network variable preview that resolves a sample request with secrets masked and reports missing/type-invalid variables.
- Sleep, bounded wait-until polling, variable extraction, assertions, conditional branching, reusable-case calls, suite/case setup and cleanup/finally hooks.
- Data-driven case iterations over bounded CSV/JSON input sets, with a stable iteration ID and per-row result.
- Case-level variables and typed inputs/outputs.
- Multi-user roles, environments, connection profiles, encrypted secrets, audit events.
- Run queue, cancellation, bounded concurrency, environment/resource locks, durable state, logs, HTML report, JUnit XML, CSV.
- Live suite progress, case/step status, run counters, timings, API latency summaries, and recent sanitized events.
- NATS Core event transport between backend components and an authenticated Rust SSE gateway; durable run events and snapshots remain in SQLite.
- Compare a completed run with the most recent successful run of the same suite revision and environment.
- Full-feature authoring, execution, and reporting in desktop browsers at a minimum viewport of 1280 × 720. Mobile and tablet support are out of scope.
- SMTP configuration and completion email.
- Trigger API, signed webhook trigger, CI command-line client, import/export UI.
- One-host Docker Compose deployment.

### Explicitly out of scope for v1

- Browser UI testing or browser launching.
- Arbitrary inline scripts for ordinary suite authors.
- Multi-host distributed workers or Kubernetes.
- Public SaaS tenancy, billing, or marketplace.
- Unreviewed third-party plugins.
- User-authored variable functions or arbitrary code in variable expressions; built-ins use a controlled, versioned function catalog.
- Direct browser credentials/connections to NATS; the Rust service performs authorization and browser fan-out.
- Message-broker test nodes (Kafka, RabbitMQ, SQS/SNS) and AsyncAPI-driven message test generation; planned after v1. AsyncAPI provides a protocol-agnostic description for message-based/event-driven APIs; see the [AsyncAPI documentation](https://www.asyncapi.com/docs/tools/generator/asyncapi-document).
- High-volume load/stress testing; use a dedicated load-testing tool, while v1 may assert response-time thresholds.
- Historical run trend dashboards, automated flaky-test quarantine, and provider/consumer contract-testing workflows; planned after the core v1 reports stabilize.
- Dedicated GraphQL and gRPC authoring experiences; v1 can send GraphQL over its HTTP request node, while gRPC support needs a separately scoped adapter.
- Destructive database migration/load testing.
- Full-text search over report bodies.
- Emailing the SQLite file, credentials, or raw unredacted artifacts.

## 5. Functional requirements

### Authoring

- FR-01: Create a suite from reusable test cases and order its cases.
- FR-02: Create and revise a test case using a visual step canvas and typed property forms.
- FR-03: Validate graph connections, required fields, types, dependency versions, timeouts, and required environment profiles before publication.
- FR-04: Reuse a node template or a specific case revision from another suite.
- FR-05: Select exact revisions or explicitly update dependencies; never silently follow “latest.”
- FR-06: Preview variable inputs, outputs, and secret references without showing secret values.
- FR-06a: Define typed custom variables at environment, run, suite, case, and data-row scopes; select a built-in function to generate a value; reference variables from API fields and other supported node inputs.
- FR-06b: Resolve and preview a sample request without network access, report missing/type-invalid references, and mask secrets.
- FR-06c: Evaluate each declared function-backed variable once per scope and reuse that value for all references; seed random test-data functions per run/iteration for repeatability.
- FR-07: Save drafts separately from published immutable revisions.
- FR-08: Export and import suites through a versioned portable bundle.
- FR-08a: Import an OpenAPI description and map operations/schemas into editable request templates; retain the source checksum and version.
- FR-08b: Define bounded case data sets from CSV/JSON and run each row as a separately reported iteration.

### Execution

- FR-09: Trigger a suite from the UI, authenticated API, or signed webhook.
- FR-10: Queue runs durably and enforce host-wide and per-suite concurrency limits.
- FR-11: Apply per-step timeout and global run deadline.
- FR-12: Support cancellation and guaranteed cleanup steps where the worker remains available.
- FR-13: Retry only when configured and safe for the operation. Never retry an ambiguous non-idempotent action automatically.
- FR-14: Record each step's input/output summary, timings, status, attempts, and diagnostic reference.
- FR-15: Recover cleanly after application restart; mark uncertain in-flight side effects as interrupted/unknown, not passed.
- FR-15a: Run suite/case setup and cleanup hooks with explicit ordering, bounded deadlines, and visible cleanup failures.
- FR-15b: Wait for an API or database condition with a deadline and polling interval; expose attempts and last observed result.
- FR-15c: Prevent conflicting runs from using a configured shared environment/resource concurrently; queue or fail fast according to policy.
- FR-15d: Rerun selected failed cases as a new run, retaining the original run and pinned revisions.
- FR-15e: Provide a CI CLI that starts a run, waits/polls for completion, downloads JUnit XML, and exits with a status reflecting the result.

### Reporting

- FR-16: Show suite, case, and step outcomes with timestamps, durations, assertions, sanitized errors, and logs.
- FR-17: Offer HTML and CSV downloads plus JUnit XML for CI systems.
- FR-18: Send a completion email through configured SMTP after final status is durable.
- FR-19: Retain summaries for 90 days and detailed logs/artifacts for 30 days by default.
- FR-20: Display live progress and statistics for active runs, reconnect from the last durable event, and show an explicit estimate label for any estimated completion time.
- FR-21: Publish live run-stat snapshots through private NATS Core subjects and forward them to authorized browser clients over SSE; refresh from SQLite snapshots on reconnect or broker failure.
- FR-22: Compare a completed run to the most recent prior PASS with the same suite revision and environment, showing status and duration/latency deltas without comparing sensitive payloads.
- FR-23: Maintain full workflow functionality on supported desktop viewports through a keyboard-accessible editor. The minimum supported viewport is 1280 × 720.

## 6. Non-functional targets

Initial targets are launch defaults, not a benchmark guarantee:

- Up to 4 active test cases on one host; configurable queue for additional work.
- UI/API availability target: 99.5% for an internally operated single host, excluding host maintenance and external dependencies.
- API reads under 500 ms p95 for ordinary suite/report metadata under a 50-user internal workload.
- Run event/report view updates within 2 seconds for active runs. Progress counts are exact; remaining-time estimates appear only when enough comparable history exists and are labeled estimates.
- Backend-to-UI status/stat updates target 2 seconds under the configured single-host concurrency. NATS Core is ephemeral; the transactional SQLite state remains authoritative.
- The product UI targets desktop viewports of at least 1280 × 720. No authoring action is available only through pointer drag-and-drop.
- No silent data loss acknowledged by the application; SQLite transactions and scheduled backups are required.
- Every worker invocation has CPU, memory, process-count, output-size, and wall-clock limits.
- Each connector documents supported server versions and is validated in CI.

## 7. Success measures

- An author can create and execute a simple API + assertion case without editing code.
- A suite author can reuse a pinned case and understand when its upstream revision changes.
- A failed run identifies the failed step and distinguishes assertion failure, infrastructure error, timeout, cancellation, and interruption.
- A running suite view updates within 2 seconds and shows completed/total cases and steps, status counts, elapsed time, active work, and safe latency summaries.
- An author can build a request from custom and built-in variables, preview the resolved request without sending it, and verify secrets are masked.
- Desktop users can create, configure, connect, validate, and publish workflows using the visual editor and keyboard-accessible controls.
- A completed run shows an eligible baseline and highlights status/timing/latency changes, or explains why no baseline exists.
- Backup restore and suite-bundle import complete with a preview and a recoverable rollback point.
- No secret value appears in UI responses, email, logs, bundle exports, or report HTML.

