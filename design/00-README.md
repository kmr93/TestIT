# Backend Automation Suite Builder — Design Pack

Version 1.2 · 7 October 2026

This pack turns the brief into an implementable, Rust-first design for a backend automation platform. It covers requirements, architecture, node behavior, persistence and portability, APIs, security, UI flows, deployment, delivery, and acceptance criteria. It also includes data-driven cases, setup/cleanup hooks, condition polling, OpenAPI-assisted authoring, safe built-in variable functions, variable preview, run comparison, CI command-line use, responsive editing, and NATS-backed live run statistics.

## Recommended product shape

Build a single-host web application with a Rust control plane and a typed, visual workflow editor. Use Python only in isolated execution workers for Playwright API requests, user-approved Python or shell assets, and the requested database and data-file adapters. Playwright does not require a Node service for this backend-only release: its Python API includes APIRequestContext for direct HTTP API testing. See the [Playwright API testing guide](https://playwright.dev/python/docs/api-testing).

Recommended stack:

- Control plane: Rust, Axum, Tokio, SQLx, SQLite.
- Web UI: React + TypeScript + React Flow with responsive desktop, tablet, and mobile authoring. React Flow supplies the interactive node/edge canvas; see its [component documentation](https://reactflow.dev/api-reference/react-flow). Mobile also gets a touch-friendly list editor for full feature coverage.
- Execution: short-lived, isolated Python worker containers; pinned Python and dependency image versions.
- Playwright: Python APIRequestContext for HTTP API nodes. Do not launch a browser in v1.
- Variables: typed custom values plus a versioned allow-list of built-in functions, resolved by the Rust control plane and previewable without sending requests.
- Real-time events: one private NATS Core server in Compose; Rust publishes/consumes run-stat events and forwards authorized updates to the browser over SSE. SQLite remains the durable source; reconnects fetch the latest snapshot.
- API authoring: import validated OpenAPI descriptions to create request templates and response-schema checks; support common API auth profiles.
- Test data: repeat cases over bounded JSON/CSV data sets; provide suite/case setup and cleanup hooks plus wait-until polling.
- Live execution: stream durable run/case/step changes and refreshed statistics to the active run view.
- SQLite: same host as the application, one authoritative file, WAL mode, migrations, and a serialized write path.
- Deployment: one Linux Docker Compose host for v1, including a private NATS Core service with no published client port. Windows development can use Docker Desktop with Linux containers.
- External data: Python adapters for MySQL/MariaDB, Cassandra, MongoDB, Delta Lake and Parquet; local filesystem and S3-compatible storage first.

Rust is a good fit for the durable API, orchestration, variable evaluation, storage, NATS integration, and security boundaries. React/TypeScript is the recommended responsive browser UI because the visual editor depends on mature canvas components and a broad UI ecosystem. Python is an intentional worker language because the brief explicitly requires Python scripts and Playwright Python, and it offers a unified connector surface. Delta Lake's delta-rs implementation itself is Rust-based.

A “fail-proof” system cannot be guaranteed when it calls external services, databases, scripts, email servers, or storage. The design instead aims for bounded failures, durable run state, clear failure classification, safe recovery, and no silent success.

## Answers to the clarification checklist

| # | Recommended answer |
|---|---|
| 1 | v1 is backend-only: APIs, scripts, database checks, and data files. No browser UI tests. Prioritize no-code authoring for normal users; retain an admin-managed library of versioned scripts for advanced cases. |
| 2 | Start on one Docker host. Cap at 4 active test cases across the host; queue additional runs. Suite cases run sequentially by default, with a setting for independent cases to run in parallel. Use environment/resource locks for suites that mutate shared test data. |
| 3 | Use Playwright's Python APIRequestContext for API calls. Do not start a browser in v1. No Node Playwright service is needed. |
| 4 | Use Rust for the API/control plane and React + TypeScript + React Flow for the visual UI. The UI is responsive across desktop/tablet/mobile; mobile gets a touch-friendly list editor for full workflow editing plus run/report views. |
| 5 | Keep suite definitions and run state in a SQLite file on the same host. Provide UI export/import as a validated portable bundle. Default retention: configurations indefinitely, run summaries 90 days, detailed logs/artifacts 30 days, with a 20 GiB artifact quota configurable by the administrator. |
| 6 | Configure named connections in the UI. Store credentials encrypted at rest with an encryption key held outside SQLite. Use role-based access for credential use and administration. |
| 7 | Run shell/Python assets in short-lived containers with resource, time, filesystem, and network limits. Never fall back to host execution. Pin Python/runtime images; no per-run package installation. |
| 8 | Support local paths and S3-compatible object storage (including MinIO) first. Add Azure, GCS, and HDFS through separately validated adapters later. |
| 9 | Produce HTML reports, JUnit XML, and CSV. Add PDF only if needed. Show live run progress/statistics and compare results to the last successful run for the same suite revision and environment. Include a no-network variable preview. Configure SMTP in the UI using TLS. Email a concise sanitized summary and authenticated report link; do not attach SQLite, secrets, or raw logs. |
| 10 | Yes. Give reusable nodes/templates and test cases immutable versions. Pin exact versions in suite revisions, show dependency changes, and reject dependency cycles. |
| 11 | Multi-user with Admin, Author, Runner, and Viewer roles. Start with local accounts and secure sessions; add OIDC SSO as a planned integration. |
| 12 | Include authenticated API triggers, signed webhooks, and a small CI command-line client in v1. Add built-in cron scheduling after the execution and recovery model has proven stable. |
| 13 | Keep structured application and run logs for 30 days by default, run summaries for 90 days, expose health plus Prometheus-compatible metrics, and stream live run counters/timings through a private NATS Core service to the Rust SSE gateway. SQLite stores durable snapshots/events; JetStream is not required for v1. |
| 14 | No formal compliance requirements were supplied. Apply encryption in transit, encrypted secrets, audit history, least privilege, and configurable retention by default. |
| 15 | Deliver all documents in this pack. No calendar MVP date was supplied; estimate 18–22 weeks for a three-engineer team with part-time QA/DevOps, subject to connector, NATS, responsive UI, and security validation. |

## Documents

1. [Product requirements](01-product-requirements.md)
2. [Architecture and execution design](02-architecture.md)
3. [Workflow nodes and reuse/versioning](03-workflows-and-nodes.md)
4. [Data model and import/export](04-data-and-portability.md)
5. [HTTP API contract](05-api-contract.md)
6. [UI/UX flows](06-ui-and-user-flows.md)
7. [Security, reporting, and operations](07-security-reporting-operations.md)
8. [Delivery, developer guide, and acceptance plan](08-delivery-and-acceptance.md)

The documents describe v1 boundaries and later options. The team should turn them into repo-level engineering decisions before implementation, especially the executor network policy and local secret-key backup procedure.

