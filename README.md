# TestIT — Backend Automation Suite Builder

TestIT is a desktop web application for authoring and running backend validation workflows. Its current implementation includes a Rust control plane, a React/TypeScript interface, a Python worker runtime, SQLite persistence, NATS outbox publication, and Docker Compose deployment.

The editor supports ordered API, database, tabular-count, and wait steps; case setup/main/cleanup groups; bounded JSON/CSV iterations; suite composition; scoped variables; run monitoring; and HTML/JUnit/CSV exports. The design pack includes additional capabilities that are still in progress. See [PROJECT_REVIEW.md](PROJECT_REVIEW.md) and [IMPLEMENTATION_TRACKER.md](IMPLEMENTATION_TRACKER.md) for current coverage and verification.

## Desktop support

The supported browser viewport starts at **1280 × 720**. The interface is designed for desktop use and does not target mobile layouts.

## Start with Docker Compose

Copy `.env.example` to `.env`, then replace the example master key, bootstrap administrator password, and worker-manager token with unique values before starting the stack. The master key must be 64 hexadecimal characters. The administrator password must meet the configured minimum. Back up the key securely; stored secrets cannot be decrypted without it.

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

The current publishable runtime supports API requests, API wait-until polling, MySQL read checks, MongoDB read checks, bounded tabular row-count checks, and sleep. Branches, reusable cases, Cassandra/scripts, suite-level setup/teardown, resource locks, portable project bundles, backup/restore, retention, SMTP notifications, schedules, and signed webhooks remain open design work. Compose and real worker-engine execution must be verified in the intended Linux deployment environment before relying on the application for production checks.
