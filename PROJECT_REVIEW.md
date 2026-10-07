# TestIT implementation review

**Reviewed:** 2026-10-07
**Design baseline:** `design/` v1.2 with desktop-only support
**Conclusion:** The repository now contains a real authoring-to-execution path for a bounded set of workflow node types. The checked-in build is not yet the complete product described in the design documents, and full acceptance is not established.

## Update in this implementation pass

- Added definition-only project ZIP export and import for suites, cases, published revisions, and connection shapes. Exports redact configured secret values, replace credential-bearing URLs, and mark connection credentials for re-entry.
- Added archive bounds and path checks, ZIP CRC checks, manifest and SHA-256 validation, revision/dependency/workflow validation, conflict preview, ID remapping, atomic merge-as-new import, and audit records for successful export/import.
- Added desktop author controls to download a bundle, preview an upload, review name conflicts and credential re-entry, and apply the import.
- Added audited connection-profile editing so an administrator can bind locally encrypted secret names to imported profiles without exposing secret plaintext.
- Added case-insensitive, workspace-scoped suite resource locks. Conflicting runs remain queued with visible wait state and a bounded deadline; renewable leases protect active runs, and expired or restart-interrupted leases become uncertain for audited administrator recovery.
- Added suite-level setup and cleanup hooks to the suite editor and executor. Setup outputs are available to case iterations; cleanup is attempted after setup or case failure and cancellation. Hooks have their own visible run/report scope, and planned steps that do not start are persisted as skipped.
- Added Cassandra connection profiles, verified-TLS read-only CQL checks and wait-until polling, with bound parameters, row/output bounds, selected columns, and isolated-worker egress validation.
- Reworked OpenAPI JSON import to validate bounded 3.0/3.1 documents, reject remote references, retain local parameters and JSON schemas, create editable case drafts instead of orphan templates, preserve the source checksum/version, and audit transactional imports. Imported requests now expose path/query parameters and validate request/response bodies against their schemas.
- Rebuilt the run monitor as a full desktop report surface with overview, timeline, and statistics tabs; exact case/node progress, live/polling/stale connection state, elapsed/run metadata, status counters, and API status/latency aggregates. The workflow node library now wraps at the supported desktop width and is hidden for read-only users.
- Added environment-provided NATS publisher credentials, a minimum password length, and a subject permission that allows publishing run-stat snapshots while denying subscriptions. Compose now requires the credentials; the control-plane log no longer prints the configured NATS URL.
- Bundles do not include run history, reports/artifacts, scripts, or a SQLite snapshot. The importer currently accepts the uncompressed ZIP format produced by this application; external deflated ZIPs are not accepted.
- Current verification for this pass: `cargo fmt --all -- --check`, `cargo check --tests --workspace --locked`, `npm run build`, `python -m compileall -q workers/python deploy/worker-manager`, `docker compose --env-file .env.example config --quiet`, and `git diff --check` passed. Compose config rendering emitted a warning because the local Docker CLI config was inaccessible. No containers or NATS server were started; Rust runtime tests and browser interaction were not run.

## Implemented in the current tree

- A desktop-only React interface with suite composition, pinned case revisions, an ordered node editor, node configuration, environment selection, run inputs, variable editing/preview, run monitoring, and report links.
- Case data iteration from bounded JSON/CSV inputs. Iterations have distinct run records, pinned row indices, and deterministic variable context.
- Suite revisions can declare typed run inputs. Publication validates the declaration, and the run API rejects missing required values, undeclared fields, and values with the wrong type before queueing.
- Publishing checks variable references against declared run/suite/case/data-row names, built-in system names, and earlier nodes in phase execution order. Literal function arguments are also evaluated for configuration errors during publication.
- Case-level setup, main, and cleanup phases, plus suite-level setup and cleanup. Setup failure skips main execution; cleanup is attempted after main/setup failure and cancellation. Suite setup outputs are passed into each case iteration, and suite cleanup is attempted after case or setup failure/cancellation. Cleanup errors remain reported and do not overwrite an earlier failed result.
- A Rust orchestrator that validates and dispatches supported nodes to a separate worker manager, validates result envelopes, persists statuses/outputs/metrics, and observes cancellation. The worker manager creates short-lived isolated containers and enforces configured target hosts and resource/output bounds.
- API wait-until polling for idempotent GET, HEAD, and OPTIONS requests, with bounded intervals/deadlines and attempt reporting.
- OpenAPI JSON 3.0/3.1 import from paste or a local file, including editable operation case drafts, common path/query/header parameters, source checksums, local component schemas, and runtime request/response JSON Schema validation.
- Run-, suite-, case-, iteration-, environment-, and step-scoped variable resolution for the implemented resolver features; previews avoid network execution and mask secret references.
- Login/session/CSRF/rate-limit/role controls, user administration, encrypted connection secrets, report endpoints, CLI execution, sequenced SQLite event replay, run progress snapshots, outbox publication, and a Prometheus text metrics endpoint.

## Remaining high-impact design gaps

### Workflow behavior

- Conditional branches and reusable case calls are absent. The visual editor is an ordered sequence, and publication rejects unsupported graph routing.
- Suite hooks currently support the same bounded node types as the case runtime, but do not yet offer branches or reusable-case calls.
- Supported nodes are API request, API/MySQL/Cassandra/MongoDB wait-until, MySQL/Cassandra read checks, MongoDB read, bounded tabular count, and sleep. Cassandra is currently limited to verified-TLS parameterized SELECTs with bounded selected-column outputs. Approved-script execution is not available in the publishable runtime.
- OpenAPI import supports JSON uploads/paste and local references. YAML, approved-URL retrieval, auth/security scheme mapping, and full OpenAPI parameter styles are not implemented.
- Tabular checks, complete response extraction/assertions, declared case input contracts for reusable calls, full static type inference across every workflow field, and SecretRef variable types do not cover the design's complete contract.

### Data, reporting, and operations

- Definition-only ZIP bundle import/export with conflict preview and transactional application is implemented. Consistent SQLite backup/restore, retention, SMTP notifications, signed webhook triggers, and schedules are absent.
- NATS has a subject-restricted authenticated outbox publisher, while SSE resumes durable events from SQLite. NATS consumer fan-out, outage behavior, and notification/outbox recovery still need acceptance work.
- Key rotation, an independent workspace-isolation audit, operational alerts/runbook, and disk/retention controls are absent or incomplete.

### Verification and release confidence

- Compilation does not establish worker runtime correctness, browser usability, secret redaction under hostile inputs, or fault recovery. Linux Compose execution and disposable connector integration remain unverified.
- The design acceptance matrix in `design/08-delivery-and-acceptance.md` is not met. The implementation tracker records the remaining work by capability.

## Verification evidence

- `cargo fmt --all`: passed.
- `cargo check --workspace --locked`: passed after fixing a compile error in phase execution. Existing warnings remain.
- `cargo check --tests --workspace --locked`: passed; this type-checks test targets but does not execute them.
- `npm run build`: passed.
- `git diff --check`: passed; Git reported line-ending normalization notices only.
- `python -m compileall -q workers/python deploy/worker-manager`: passed.
- The targeted resource-lock `cargo test` build remained in compilation and was stopped before test execution. Compose startup, Docker worker execution, connector integration, and browser walkthrough were not run during this implementation pass.
