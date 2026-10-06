# TestIT — Backend Automation Suite Builder

TestIT is an enterprise-grade backend automation testing platform designed for deterministic, fail-proof execution across HTTP APIs, relational databases (MySQL, MariaDB), NoSQL databases (Cassandra, MongoDB), and tabular data files (Parquet, Delta Lake).

Built in strict compliance with the architecture pack in `design/`.

---

## 🏗️ Architecture & Component Overview

```
                      +---------------------------------------+
                      |       Responsive Web UI               |
                      |   (React + TypeScript + Vite)         |
                      +-------------------+-------------------+
                                          |
                      +-------------------v-------------------+
                      |      Rust Control Plane (Axum)        |
                      |  - SQLite (WAL mode & Outbox)         |
                      |  - AES-256-GCM Secret Encryption      |
                      |  - Deterministic Variable Evaluator   |
                      |  - SSE Real-time Progress Gateway     |
                      +---------+-------------------+---------+
                                |                   |
        +-----------------------v----+        +-----v----------------------+
        |      Private NATS Core     |        |    Isolated Python Worker  |
        |  - Ephemeral stats fan-out |        |  - Playwright APIRequest   |
        |  - Internal Docker network |        |  - MySQL, Mongo, Parquet   |
        +----------------------------+        +----------------------------+
```

---

## 🚀 Quickstart with Docker (Primary Setup)

Docker Compose is the primary deployment target. It brings up the private NATS Core message bus, the Rust Control Plane, and the Web UI in isolated networks.

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
npm install
npm run build
npm run dev
```

### CI Command-Line Client (`testit-cli`)
```bash
cd apps/cli
cargo run -- --server http://localhost:8080 run --suite <suite_revision_id> --env <env_id> --junit report.xml
```

---

## 🛡️ Fail-Proof Safeguards

- **Transactional Outbox & SQLite WAL:** Atomic status updates and outbox commits. Dropped NATS events never corrupt durable state.
- **Worker Crash Resilience:** Ephemeral workers emit versioned NDJSON frames. Unhandled crashes emit error envelopes preventing coordinator hangs.
- **Zero-Network Variable Preview:** Deterministic resolution of variables without hitting external endpoints or resolving live secrets.
- **Role-Based Encrypted Secrets:** Credentials encrypted at rest with AES-256-GCM using an external master key. Never emitted in browser storage or SSE logs.

---

## 📋 Implementation Tracker

For live development status, verification records, and phase checklists, see:
👉 [IMPLEMENTATION_TRACKER.md](IMPLEMENTATION_TRACKER.md)
