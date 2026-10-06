# TestIT — Backend Automation Suite Builder

TestIT is an early prototype for authoring backend test workflows across HTTP APIs, relational databases (MySQL, MariaDB), NoSQL databases (Cassandra, MongoDB), and tabular data files (Parquet, Delta Lake).

The repository includes a Rust control plane, React desktop UI, and Python worker adapters. It is not production-ready: authentication/RBAC and worker dispatch are incomplete, and run statuses must not be treated as evidence that checks executed. See [PROJECT_REVIEW.md](PROJECT_REVIEW.md) for the implementation review against `design/`.

---

## 🏗️ Current component map

```
  Desktop web UI (React + TypeScript + Vite)
                     |
        Rust control plane (Axum + SQLite)
             /                       \
  NATS config/outbox code       Python worker adapters
  (SSE still polls SQLite)      (not called by orchestrator)
```

This map describes code present in the repository. The NATS-to-SSE and control-plane-to-worker paths are not yet integrated end to end.

---

## 🚀 Quickstart with Docker (Primary Setup)

Docker Compose is the development deployment target. It starts NATS, the Rust control plane, and the web UI; it does not currently prove end-to-end worker execution.

Before the first launch, set `MASTER_KEY_HEX` to a unique 64-character hex key in your shell or ignored `.env` file. Keep the key backed up securely; losing it makes stored secrets unreadable. Compose now refuses to start without this value.

### 1. Launch the Stack

```bash
# Build and start all services
docker compose up --build -d
```

### 2. Access the Applications

- **Web UI:** [http://localhost:3000](http://localhost:3000)
- **API & Health:** [http://localhost:8080/health/ready](http://localhost:8080/health/ready)
- **API Documentation & Endpoints:** `/api/v1/*`

### 3. Check Health

```bash
curl http://localhost:8080/health/live
curl http://localhost:8080/health/ready
```

---

## 🛠️ Local Development Setup

### Rust Control Plane
```bash
cd services/control-plane
cargo check
cargo test
cargo run
```

### Web UI
```bash
cd apps/web
npm ci
npm run build
npm run dev
```

### CI Command-Line Client (`testit-cli`)
```bash
cd apps/cli
cargo run -- --server http://localhost:8080 run --suite <suite_revision_id> --env <env_id> --junit report.xml
```

---

## Current implementation

- The UI supports desktop browsers from 1280 × 720 and uses a keyboard-operable ordered-step editor. Graph editing is not implemented.
- The Python worker contains a versioned protocol and connector adapters, but the Rust orchestrator does not call it yet.
- Variable preview and AES-256-GCM secret encryption code exist. Authentication, authorization, and key rotation are not implemented.
- HTML, JUnit, and CSV report endpoints exist; report and event-stream acceptance has not been verified end to end.

---

## 📋 Implementation Tracker

For live development status, verification records, and phase checklists, see:
👉 [IMPLEMENTATION_TRACKER.md](IMPLEMENTATION_TRACKER.md)
