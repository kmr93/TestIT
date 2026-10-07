# TestIT implementation tracker

**Design baseline:** `design/` v1.2, October 2026
**Product target:** desktop browsers at 1280 × 720 or wider
**Status:** substantial implementation is present, but full design acceptance is not met. See [PROJECT_REVIEW.md](PROJECT_REVIEW.md) for the current code-backed review.
**Last updated:** 2026-10-07

## Current implementation

| Area | Implemented | Still required for design acceptance |
|---|---|---|
| Desktop authoring | React/TypeScript desktop UI; suite composer pins case revisions; ordered case editor supports API, wait-until, MySQL, Cassandra, MongoDB, tabular count and sleep nodes; case/suite lifecycle hooks; JSON/CSV iterations; typed suite run-input declarations; scoped variables and preview; OpenAPI 3.0/3.1 JSON upload/paste creates editable case drafts with parameters and schema checks | Branch and reusable-case execution; full authentication-profile mapping, YAML/approved-URL OpenAPI import, complete extraction/assertion coverage; accessible browser review at the supported minimum viewport |
| API and identity | Rust/Axum API; Argon2 login, expiring/revocable sessions, CSRF on mutations, rate limiting, role gates, user administration, secret encryption; workspace checks on API resources; audited connection profile editing and secret rebinding | Full object-level isolation audit, key rotation and security acceptance tests |
| Run execution | Durable suite/case/step records; bounded case concurrency; isolated worker-manager dispatch; result validation; bounded worker runtime; run cancellation; failed-case rerun pins; API/MySQL/Cassandra/MongoDB wait-until; typed suite run inputs; publish-time variable reference validation; workspace-scoped named resource locks with queued wait deadlines, renewable leases, restart/expiry uncertainty, audited release, and admin recovery; suite-level setup and cleanup hooks with output sharing, failure/cancellation cleanup, and skipped-step accounting | Restart/recovery guarantees across every interruption point; Linux Docker integration and connector compatibility matrix |
| Events and monitoring | Durable sequenced SQLite run events and progress snapshots; SSE resume; outbox publisher; full desktop run monitor with overview, timeline, and statistics views; exact case/node counts, active case/step, polling fallback indicator, run metadata, and aggregate API status/p50/p95; Prometheus text metrics endpoint | NATS-backed fan-out and outage behavior acceptance; authorized reconnect tests; alerts and load evidence |
| Reports and CLI | HTML/JUnit/CSV exports; bounded CLI run wait and exit status | Report redaction/access acceptance; SMTP notifications and delivery deduplication |
| Portability and operations | Compose deployment, health endpoints, required encryption key, NATS and worker-manager services; definition-only ZIP export/import with checksums, secret redaction/re-entry markers, conflict preview, UUID remapping, transactional import, and audit events | Consistent backup/restore; retention controls; NATS authentication/ACLs; operational runbook |

## Design requirements still open

- Branch nodes and reusable case calls are not supported by the ordered editor or runtime. Publication rejects unsupported routing.
- Supported execution types include `api.request`, `wait.until` (read-only API and parameterized MySQL/Cassandra SELECT or MongoDB count), `db.mysql`, `db.cassandra`, `db.mongodb`, `data.tabular`, and `sleep.wait`. Cassandra support is bounded to verified-TLS SELECTs and selected output columns; approved script assets are not yet publishable through the workflow runtime.
- OpenAPI import accepts bounded JSON 3.0/3.1 documents with local references. It validates operation IDs, path/operation parameters and schemas, creates editable API case drafts, records the source checksum/version, and audits import. YAML, approved-URL fetch, security-scheme mapping, and complex parameter serialization are still open.
- `data.tabular` is limited to the adapter's bounded read/count behavior; row-level assertions, extraction, and broad storage-provider coverage remain open.
- Variable definitions support the implemented typed literals and allow-listed functions, but declared case inputs, complete static type checking, and SecretRef variables are incomplete.
- Case cleanup runs after setup/main failure and cancellation. Suite setup runs once before case iterations; suite cleanup runs once after completion, setup failure, or cancellation. Hook steps use the shared worker/runtime contracts, suite setup outputs flow into case contexts, and unstarted planned steps are recorded as skipped. Resource locks are workspace-scoped and exclusive by case-insensitive name; a contending run stays queued until release or its configured 1–3600 second wait deadline. Leases renew during execution, and stale locks become uncertain after expiry or control-plane restart until an administrator reviews and releases them with an audit reason.
- Project ZIP bundles now support definition-only export/import; run history, artifacts, script assets, consistent SQLite backup/restore, and retention remain unsupported. SMTP completion mail, signed webhooks, and schedule triggers are not implemented.
- NATS publication exists through an outbox, while the browser event stream resumes from SQLite. NATS consumer fan-out, authentication/ACL configuration, and outage/recovery acceptance remain open.
- Authentication and route roles exist; independent workspace-isolation/security review and key rotation remain open.
- Docker Compose end-to-end runs, Linux worker-engine behavior, connector integration, and full browser interaction have not been verified here.

## Verification recorded for this implementation pass

- `cargo fmt --all`: passed.
- `cargo check --workspace --locked`: passed after the final Rust compile fix; existing unused/dead-code warnings remain.
- `npm run build` in `apps/web`: passed.
- `git diff --check`: passed (Git reported line-ending normalization notices only).
- `python -m compileall -q workers/python deploy/worker-manager`: passed.
- `cargo check --tests --workspace --locked`: passed, including test-target type checking.
- Suite-hook slice verification: `cargo check --workspace --locked`, `cargo check --tests --workspace --locked`, `npm run build` in `apps/web`, and `git diff --check` passed. The current pass type-checks tests but has not run Rust runtime tests.
- A targeted `cargo test -p testit-control-plane resource_lock_tests::` build did not reach test execution; it remained at control-plane compilation and was stopped. Runtime test results are therefore unverified.
- Compose/Docker execution was not run in this pass.

These checks establish formatting, compilation, and frontend bundling only. They do not establish the remaining design acceptance criteria.
