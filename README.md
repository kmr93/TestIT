# TestIT — Backend Automation Suite Builder

TestIT is a desktop web application for authoring and running backend validation workflows. Its current implementation includes a Rust control plane, a React/TypeScript interface, a Python worker runtime, SQLite persistence, NATS outbox publication, and Docker Compose deployment.

The editor supports ordered API, database, tabular-count, and wait steps; case setup/main/cleanup groups; bounded JSON/CSV iterations; suite composition; scoped variables; run monitoring; HTML/JUnit/CSV exports; and definition-only project bundles with import preview. Imported connection profiles can be edited to bind local encrypted secrets. The design pack includes additional capabilities that are still in progress. See [PROJECT_REVIEW.md](PROJECT_REVIEW.md) and [IMPLEMENTATION_TRACKER.md](IMPLEMENTATION_TRACKER.md) for current coverage and verification.

## Desktop support

The supported browser viewport starts at **1280 × 720**. The interface is designed for desktop use and does not target mobile layouts.

## Start with Docker Compose

Copy `.env.example` to `.env`, then replace the example master key, bootstrap administrator password, worker-manager token, and NATS publisher password with unique values before starting the stack. The master key must be 64 hexadecimal characters. The administrator password must meet the configured minimum; the NATS password must contain at least 32 characters. Back up the key securely; stored secrets cannot be decrypted without it.

```sh
docker compose up --build -d
```

The default local addresses are:

- Web interface: [http://localhost:3000](http://localhost:3000)
- Readiness: [http://localhost:8080/health/ready](http://localhost:8080/health/ready)
- Liveness: [http://localhost:8080/health/live](http://localhost:8080/health/live)
- Prometheus metrics: [http://localhost:8080/metrics](http://localhost:8080/metrics)

Sign in with the bootstrap administrator credentials configured in `.env`. Review the Compose configuration and network/worker target allow-list before exposing the stack beyond a local development machine.

## Local development

```sh
# Control plane
cargo check --workspace --locked

# Web application
cd apps/web
npm ci
npm run build

# Worker syntax validation
cd ../..
python -m compileall -q workers/python deploy/worker-manager
```

The CLI can trigger and wait for a suite run against a running API:

```sh
cd apps/cli
cargo run -- --server http://localhost:8080 run --suite <suite_revision_id> --env <environment_id> --junit report.xml
```

## Current limits

The current publishable runtime supports API requests, API/MySQL/Cassandra/MongoDB wait-until polling, MySQL and Cassandra read checks, MongoDB read checks, bounded tabular row-count checks, sleep, case and suite lifecycle hooks, and workspace-scoped named resource locks with bounded waiting and audited recovery. OpenAPI 3.0/3.1 JSON import creates editable API case drafts with source checksums, parameters, and JSON Schema validation; Cassandra is limited to verified-TLS parameterized SELECTs with bounded selected-column outputs. The private NATS outbox publisher uses environment-supplied credentials and is restricted to run-stat subjects; the browser still resumes events from SQLite. Branches, reusable cases, approved scripts, OpenAPI YAML/URL import and security-scheme mapping, SQLite backup/restore, retention, SMTP notifications, schedules, and signed webhooks remain open design work. Bundles currently include definitions and connection shapes only, and the importer accepts stored ZIP entries produced by this application. Compose and real worker-engine execution must be verified in the intended Linux deployment environment before relying on the application for production checks.
