# Delivery Plan, Developer Guide, and Acceptance

## 1. Delivery estimate

No MVP date was supplied. With the added typed variable/function system, responsive full mobile authoring, variable preview, run comparison, private NATS Core service, data-driven execution, OpenAPI import, wait-until behavior, CI CLI, and environment locks, a realistic first release is 18–22 calendar weeks with two backend engineers (Rust control plane and Python execution/connectors), one frontend engineer, and part-time QA/DevOps/security support. A single engineer should budget approximately 8–11 months depending on connector and deployment experience.

The estimate assumes the host is a single Linux Docker Compose installation and that connector acceptance is limited to a declared, tested server/version matrix.

## 2. Milestones

| Weeks | Milestone | Exit evidence |
|---|---|---|
| 1–2 | Product/technical foundation | ADRs, workflow JSON schema, threat model, CI skeleton, connector support matrix |
| 3–4 | Control plane and storage | Rust service, auth/RBAC, SQLite migrations, asset drafts/revisions, audit |
| 5–7 | Authoring foundation | Responsive desktop/tablet/mobile editor shell, typed variables, built-in function catalog, request variable picker/preview, APIRequestContext worker, OpenAPI import preview |
| 8–11 | Runtime and connectors | Deterministic variable resolver, auth profiles, CSV/JSON iterations, setup/cleanup, wait-until, locks, script assets, DB/data connectors |
| 12–14 | Live events and reporting | Private NATS Core + SQLite outbox/snapshots, SSE gateway/reconnect, live stats, run comparison, HTML/JUnit/CSV, SMTP, CI CLI |
| 15–17 | Import/export and admin | Validated bundle flow, connections/secrets UI, backup/restore, retention, Compose deployment and NATS auth/ACLs |
| 18–20 | Hardening and pilot | Threat-focused review, responsive/accessibility review, connector matrix, fault recovery, operator runbook, pilot fixes |
| 21–22 | Contingency | Connector incompatibility, mobile-editor refinement, performance tuning, release packaging, deployment rehearsal |

Do not start the pilot until worker isolation, secret redaction, backup/restore, and interrupted-run behavior meet acceptance criteria.

## 3. Suggested repository layout

    /apps/web                 React + TypeScript UI
    /apps/cli                 CI command-line client
    /services/control-plane   Rust Axum application
    /schemas/variables        typed variable and function-catalog schemas
    /deploy/nats              pinned NATS Core config and subject permissions
    /workers/python           Python node protocol and adapters
    /schemas                  workflow, API, worker envelopes
    /migrations               SQLite migrations
    /deploy                   Docker Compose, reverse proxy examples
    /docs                     product, architecture, security, operator docs
    /fixtures                 safe sample suites and connector fixtures

Keep schemas as the source of truth. Generate API types for frontend and worker protocol validators. Do not share runtime state by letting the worker write SQLite; workers return result envelopes to the control plane.

## 4. Developer workflow

1. Install pinned Rust toolchain, Node package manager/runtime, Python runtime, and Docker engine versions from the repository's toolchain files.
2. Start SQLite-backed app services, a pinned NATS Core container, and local worker dependencies through the development Compose profile.
3. Run Rust formatting, linting, unit/integration checks; UI lint/type checks; Python formatter/type/lint checks.
4. Run schema compatibility checks and SQLite migration tests on both a fresh database and a prior supported version.
5. Use local disposable test databases for connector integration checks. Never use production credentials in developer or CI environments.
6. Build worker images from lockfiles, scan dependencies, and record image digests in release metadata.
7. Keep a sample suite that covers OpenAPI-derived API request, custom and function-backed variables, no-network preview, data-driven iterations, extraction, DB read, wait-until, branch, reusable case, setup/cleanup, failure, cancellation, live stats, comparison, and export/import.

## 5. Definition of done

For each node type:

- typed schema, validation rules, input/output contract, sensitivity classification;
- timeout, output-size, retry, and redaction behavior;
- adapter implementation and supported server matrix;
- unit tests with fakes plus integration tests against disposable targets;
- documentation and example node configuration;
- report rendering and import/export compatibility.

For each API endpoint:

- authentication/authorization, validation, rate limits where needed, stable errors, audit behavior, pagination/idempotency, API schema update.

For each UI flow:

- keyboard path, screen-reader labels, loading/empty/error states, stale revision handling, and permission-denied state.

## 6. Acceptance plan

### Authoring/reuse

- A user can assemble an API request, response assertion, database read check, sleep, branch, and cleanup flow without writing code.
- Built-in functions are typed, allow-listed, versioned, and deterministic for a run/iteration; repeated references to one variable resolve to the same value.
- Typed custom variables can be referenced from supported API fields and node inputs; unresolved names and type mismatches are caught before execution.
- Variable preview renders an API request from sample context without network access, script execution, DB connection, or secret resolution.
- An author can import a supported OpenAPI description, preview generated operations, create editable request templates, and validate response schemas.
- A bounded CSV/JSON data set creates separately reported case iterations with unique iteration IDs and input-row references.
- Setup and cleanup hooks execute in the documented order; cleanup failures remain visible and never turn a failed run into PASS.
- A wait-until node stops at its deadline and reports attempts and a sanitized last observation.
- Invalid variables, graph cycles, missing environment bindings, unapproved scripts, and dependency cycles are rejected before publication.
- Updating a reusable case does not alter a published suite; suite revision shows and can explicitly adopt the new case revision.

### Execution/recovery

- Host concurrency never exceeds configured cap; excess work queues durably.
- Worker timeout, forced process termination, application restart, and cancellation each produce a visible non-PASS outcome.
- An interrupted non-idempotent action is not automatically replayed.
- Logs, response bodies, and artifacts respect configured size limits and secret redaction.
- Run event replay from a reconnecting UI shows no missing or reordered durable events.
- The active run page refreshes progress within 2 seconds, shows exact case/node status counts and current work, and marks ETA as estimated or indeterminate as appropriate.
- On a dropped SSE connection, the UI indicates stale state and recovers from the last event ID without duplicating terminal state transitions.
- A conflicting run waits on its declared environment/resource lock and receives a visible timeout if the lock is not released.
- The CI CLI returns 0 only on PASS, returns non-zero for other final statuses, and can download the matching JUnit report.
- NATS Core carries versioned run-stat snapshots to the SSE gateway; the gateway enforces run authorization and the browser has no NATS credentials.
- NATS outage or dropped messages do not corrupt run state; outbox recovery and a fresh SQLite snapshot restore the current view, with polling/stale indicators during outage.
- A same-revision/same-environment run comparison selects the latest prior PASS and reports status/timing/latency differences without record-level payload comparisons.

### Security

- A normal worker cannot read unrelated host files, Docker socket, SQLite file, or other runs' secrets.
- An unapproved imported script cannot run.
- Egress to an unconfigured destination is denied; allowed private target access works.
- No credential appears in browser responses after save, reports, emails, logs, or exported bundles.
- Role tests confirm Viewer cannot trigger; Runner cannot edit or administer; Author cannot read secret plaintext; Admin operations are audited.

### Data and reports

- Bundle import preview identifies conflicts and unresolved secrets; malformed/oversized bundles are rejected without changing live data.
- Failed import restores/retains the pre-import state.
- SQLite snapshot passes integrity validation; backup restore procedure is rehearsed.
- HTML report escapes hostile markup; JUnit XML parses; CSV exports consistent case/step status.
- Completion email is sent once logically despite retry, contains a secure report link, and remains queued during SMTP outage.

### Operations

- Compose install/restart/upgrade succeeds from a clean host.
- Readiness correctly fails when SQLite/artifact storage/worker engine is unavailable.
- Disk quota, retention, stale queue, failed backup, and mail backlog are visible/alertable.
- Pilot load test at 4 active test cases remains within the target host's CPU, memory, and SQLite latency envelope.
- Live statistics refresh load remains bounded at the configured concurrency and does not delay durable status writes or worker completion.
- The full authoring workflow works at desktop, tablet, and mobile viewport sizes; mobile users can add/configure/reorder/connect/validate/publish without drag-and-drop.

## 7. Principal risks and responses

| Risk | Response |
|---|---|
| SQLite becomes a bottleneck | Single-host v1, short transactions, serialized writer, bounded events; move to PostgreSQL before multi-host workers. |
| Script escapes expected boundaries | Approved assets, short-lived least-privilege containers, deny-by-default egress, no host execution; stronger sandbox for hostile tenants. |
| Retries duplicate remote side effects | Zero retry by default; explicit idempotency and interrupted/unknown status. |
| Reports leak sensitive data | Redaction at worker boundary, safe default excerpts, HTML escaping, access-controlled artifacts, secret-free email/export. |
| Driver/server compatibility differs | Publish a tested version matrix; connector-specific integration tests; do not advertise untested versions. |
| No-code promise conflicts with scripts/raw SQL | Keep scripts admin-approved and reusable; make structured nodes the normal route; label advanced code-backed options honestly. |
| SQLite backup omits WAL state or encryption key | Use a consistent backup API, separately back up the key, and rehearse restore. |
| NATS event loss or outage makes a live screen stale | Keep SQLite authoritative, publish through an outbox, sequence messages, snapshot on reconnect, and fall back to bounded polling. |
| Mobile canvas is unusable on a narrow viewport | Provide equivalent list-based editing and explicit connect/reorder controls; test full authoring on touch devices. |

## 8. Open engineering decisions before coding

These should be resolved in ADRs during weeks 1–2:

1. Exact Linux distribution and supported Docker engine versions.
2. Secret-key source: OS-protected local key file versus organization KMS.
3. Egress enforcement mechanism and DNS rebinding protection.
4. Supported server versions for each connector and storage provider.
5. Whether any destructive setup/cleanup node is allowed in v1 and which roles can publish it.
6. Whether report artifacts may include record-level data and how workspace owners classify/redact it.
7. OIDC provider and timeline.

## 9. Post-v1 roadmap candidates

Prioritize these from actual pilot usage rather than adding them all to the first release:

1. Built-in schedules and richer trigger policies after queue recovery and locking are stable.
2. Event-driven workflows: Kafka, RabbitMQ, SQS/SNS, webhook receivers, and AsyncAPI import for message channels/schemas.
3. Historical quality analytics: suite pass-rate and duration trends, failure clustering, flaky-test detection, and quarantine with an explicit expiration/review policy.
4. Provider/consumer contract testing and contract artifact comparison for service teams that need compatibility gates.
5. GraphQL-specific schema/introspection helpers and gRPC nodes where user demand justifies dedicated adapters.
6. Dedicated performance/load execution with separate resource pools and guardrails. The v1 API latency assertions and observed request rate are diagnostic statistics, not a load-testing substitute.

Keep browser UI testing out of scope unless the product direction changes; the current target remains backend systems and data services.

