# TestIT implementation tracker

**Design baseline:** `design/` v1.2, October 2026
**Product target:** desktop browsers at 1280 × 720 or wider
**Status:** substantial implementation is present, but full design acceptance is not met. See [PROJECT_REVIEW.md](PROJECT_REVIEW.md) for the current code-backed review.
**Last updated:** 2026-10-07

## Current implementation

| Area | Implemented | Still required for design acceptance |
|---|---|---|
| Desktop authoring | React/TypeScript desktop UI; suite composer pins case revisions; ordered case editor supports API, wait-until, MySQL, MongoDB, tabular count and sleep nodes; case setup/main/cleanup phases; JSON/CSV iterations; typed suite run-input declarations; scoped variables and preview | Branch and reusable-case execution; complete extraction/assertion coverage; accessible browser review at the supported minimum viewport |
| API and identity | Rust/Axum API; Argon2 login, expiring/revocable sessions, CSRF on mutations, rate limiting, role gates, user administration, secret encryption; workspace checks on API resources | Full object-level isolation audit, key rotation and security acceptance tests |
| Run execution | Durable suite/case/step records; bounded case concurrency; isolated worker-manager dispatch; result validation; bounded worker runtime; run cancellation; failed-case rerun pins; API/MySQL/MongoDB wait-until; typed suite run inputs; publish-time variable reference validation | Restart/recovery guarantees across every interruption point; environment/resource locks; Linux Docker integration and connector compatibility matrix |
| Events and monitoring | Durable sequenced SQLite run events and progress snapshots; SSE resume; outbox publisher; active case/step and counts in monitor; Prometheus text metrics endpoint | NATS-backed fan-out and outage behavior acceptance; authorized reconnect tests; alerts and load evidence |
| Reports and CLI | HTML/JUnit/CSV exports; bounded CLI run wait and exit status | Report redaction/access acceptance; SMTP notifications and delivery deduplication |
| Portability and operations | Compose deployment, health endpoints, required encryption key, NATS and worker-manager services | Validated project bundle import/export; consistent backup/restore; retention controls; NATS authentication/ACLs; operational runbook |

## Design requirements still open

- Branch nodes and reusable case calls are not supported by the ordered editor or runtime. Publication rejects unsupported routing.
- Supported execution types are `api.request`, `wait.until` (read-only API, MySQL SELECT, or MongoDB count), `db.mysql`, `db.mongodb`, `data.tabular`, and `sleep.wait`. Cassandra and script nodes are not publishable through this implementation.
- `data.tabular` is limited to the adapter's bounded read/count behavior; row-level assertions, extraction, and broad storage-provider coverage remain open.
- Variable definitions support the implemented typed literals and allow-listed functions, but declared case inputs, complete static type checking, and SecretRef variables are incomplete.
- Cleanup is supported per case iteration. Suite-level setup/teardown and resource locks are not implemented.
- Project ZIP bundles, consistent SQLite backup/restore, retention, SMTP completion mail, signed webhooks, and schedule triggers are not implemented.
- NATS publication exists through an outbox, while the browser event stream resumes from SQLite. NATS consumer fan-out, authentication/ACL configuration, and outage/recovery acceptance remain open.
- Authentication and route roles exist; independent workspace-isolation/security review and key rotation remain open.
- Docker Compose end-to-end runs, Linux worker-engine behavior, connector integration, and full browser interaction have not been verified here.

## Verification recorded for this implementation pass

- `cargo fmt --all`: passed.
- `cargo check --workspace --locked`: passed after the final Rust compile fix; existing unused/dead-code warnings remain.
- `npm run build` in `apps/web`: passed.
- `python -m compileall -q workers/python deploy/worker-manager`: passed.
- Full test suites and Compose/Docker execution were not run in this pass.

These checks establish formatting, compilation, and frontend bundling only. They do not establish the remaining design acceptance criteria.
