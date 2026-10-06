# TestIT implementation review

**Reviewed:** 2026-10-07
**Design baseline:** `design/` v1.2 with desktop-only support
**Conclusion:** The repository now contains a real authoring-to-execution path for a bounded set of workflow node types. The checked-in build is not yet the complete product described in the design documents, and full acceptance is not established.

## Implemented in the current tree

- A desktop-only React interface with suite composition, pinned case revisions, an ordered node editor, node configuration, environment selection, run inputs, variable editing/preview, run monitoring, and report links.
- Case data iteration from bounded JSON/CSV inputs. Iterations have distinct run records, pinned row indices, and deterministic variable context.
- Suite revisions can declare typed run inputs. Publication validates the declaration, and the run API rejects missing required values, undeclared fields, and values with the wrong type before queueing.
- Publishing checks variable references against declared run/suite/case/data-row names, built-in system names, and earlier nodes in phase execution order. Literal function arguments are also evaluated for configuration errors during publication.
- Case-level setup, main, and cleanup phases. Setup failure skips main execution; cleanup is attempted after main/setup failure and cancellation. Cleanup errors remain reported and do not overwrite an earlier failed result.
- A Rust orchestrator that validates and dispatches supported nodes to a separate worker manager, validates result envelopes, persists statuses/outputs/metrics, and observes cancellation. The worker manager creates short-lived isolated containers and enforces configured target hosts and resource/output bounds.
- API wait-until polling for idempotent GET, HEAD, and OPTIONS requests, with bounded intervals/deadlines and attempt reporting.
- Run-, suite-, case-, iteration-, environment-, and step-scoped variable resolution for the implemented resolver features; previews avoid network execution and mask secret references.
- Login/session/CSRF/rate-limit/role controls, user administration, encrypted connection secrets, report endpoints, CLI execution, sequenced SQLite event replay, run progress snapshots, outbox publication, and a Prometheus text metrics endpoint.

## Remaining high-impact design gaps

### Workflow behavior

- Conditional branches and reusable case calls are absent. The visual editor is an ordered sequence, and publication rejects unsupported graph routing.
- Suite-level setup and teardown and environment/resource locks are absent. Cleanup support is at case-iteration scope.
- Supported nodes are API request, API/MySQL/MongoDB wait-until, MySQL read, MongoDB read, bounded tabular count, and sleep. Cassandra and approved-script execution are not available in the publishable runtime.
- Tabular checks, complete response extraction/assertions, declared case input contracts for reusable calls, full static type inference across every workflow field, and SecretRef variable types do not cover the design's complete contract.

### Data, reporting, and operations

- Portable ZIP bundle import/export with conflict preview and transactional application is absent.
- Consistent SQLite backup/restore, retention, SMTP notifications, signed webhook triggers, and schedules are absent.
- NATS has an outbox publisher, while SSE resumes durable events from SQLite. NATS consumer fan-out, auth/ACLs, outage behavior, and notification/outbox recovery still need acceptance work.
- Key rotation, an independent workspace-isolation audit, operational alerts/runbook, and disk/retention controls are absent or incomplete.

### Verification and release confidence

- Compilation does not establish worker runtime correctness, browser usability, secret redaction under hostile inputs, or fault recovery. Linux Compose execution and disposable connector integration remain unverified.
- The design acceptance matrix in `design/08-delivery-and-acceptance.md` is not met. The implementation tracker records the remaining work by capability.

## Verification evidence

- `cargo fmt --all`: passed.
- `cargo check --workspace --locked`: passed after fixing a compile error in phase execution. Existing warnings remain.
- `npm run build`: passed.
- `python -m compileall -q workers/python deploy/worker-manager`: passed.
- Full tests, Compose startup, Docker worker execution, connector integration, and browser walkthrough were not run during this implementation pass.
