# HTTP API Contract

Base path: /api/v1. JSON is UTF-8. All requests require authentication except health probes. Mutating requests require CSRF protection for cookie sessions. All externally visible IDs are UUIDs. Unknown fields are rejected for published definitions unless the schema explicitly permits extensions.

## 1. Core resources

| Method | Path | Purpose |
|---|---|---|
| GET | /health/live | Process liveness |
| GET | /health/ready | SQLite, artifact store, worker engine readiness |
| GET | /api/v1/me | Current identity and permissions |
| GET/POST | /api/v1/assets | List/create asset drafts |
| GET/PATCH | /api/v1/assets/{asset_id}/draft | Read/update a draft |
| POST | /api/v1/assets/{asset_id}/publish | Validate and create immutable revision |
| GET | /api/v1/assets/{asset_id}/revisions | List revisions and dependency changes |
| GET | /api/v1/assets/{asset_id}/revisions/{revision_id} | Read immutable revision |
| POST | /api/v1/runs | Trigger a suite revision |
| GET | /api/v1/runs | Filter/paginate runs |
| GET | /api/v1/runs/{run_id} | Run summary |
| GET | /api/v1/runs/{run_id}/stats | Current run progress and aggregate statistics snapshot |
| POST | /api/v1/runs/{run_id}/cancel | Request cancellation |
| POST | /api/v1/runs/{run_id}/rerun-failed | Start a new run for failed case iterations using the same pinned suite revision |
| GET | /api/v1/runs/{run_id}/events | SSE event stream |
| GET | /api/v1/runs/{run_id}/report | HTML report |
| GET | /api/v1/runs/{run_id}/exports/{format} | html, junit, or csv download |
| GET/POST | /api/v1/connections | List/create connection profiles |
| POST | /api/v1/connections/{id}/test | Run a bounded connectivity check |
| POST | /api/v1/imports/validate | Upload and validate a bundle |
| POST | /api/v1/imports/{id}/commit | Commit a reviewed import |
| POST | /api/v1/exports | Create a portable export |
| GET | /api/v1/exports/{id} | Download export |
| GET/POST | /api/v1/webhooks | Manage signed webhook triggers |
| POST | /api/v1/specifications/openapi/validate | Validate an uploaded or approved OpenAPI description and return import preview |
| POST | /api/v1/specifications/openapi/import | Create editable operation templates from an approved validated specification |
| POST | /api/v1/variables/preview | Resolve sample variables and render an API request without network access |
| GET | /api/v1/runs/{run_id}/comparison | Compare a run with the latest eligible successful baseline |

Use cursor pagination for runs and events. Apply authorization to every object lookup, not only at the UI.

## 2. Publish workflow

POST /api/v1/assets/{asset_id}/publish accepts:

    {
      "expected_draft_version": 12,
      "change_note": "Add duplicate-record check",
      "dependency_update_policy": "pin_current"
    }

The server validates the definition, resolves exact dependency revisions, computes a canonical checksum, allocates the next revision number, and returns the revision ID. A stale expected_draft_version returns HTTP 409.

## 3. Run trigger

POST /api/v1/runs accepts:

    {
      "suite_revision_id": "uuid",
      "environment_id": "uuid",
      "inputs": {
        "customer_id": {"type": "string", "value": "C-104"}
      },
      "variable_overrides": {
        "run.request_tag": {"type": "string", "value": "smoke-2026-10-07"}
      },
      "notification": {"email_on_completion": true},
      "idempotency_key": "caller-generated-unique-key"
    }

Response: HTTP 202 with run_id, status=QUEUED, created_at, and links for status/events/report. Repeating an idempotency key with the same payload returns the same run; reusing it with a different payload returns 409.

Webhook requests include timestamp, nonce, key ID, and HMAC signature over the raw body. Reject invalid signatures, stale timestamps, and reused nonces. Webhooks can trigger only a pre-authorized suite/environment pair and cannot carry arbitrary secret values.

## 4. Variable preview and evaluation

Variable definitions are structured schema values, not executable strings. A function-backed custom variable is represented by its type and allow-listed function ID/arguments; the run manifest pins the catalog version and seed. The UI inserts typed references such as {{case.order_id}} into supported fields. The server rejects unknown names, scopes, types, or function IDs.

POST /api/v1/variables/preview accepts the draft/revision, selected environment, sample inputs/data-row values, and target request node. The server uses the same Rust resolver as execution and returns resolved non-secret values plus a redacted request preview and validation errors. The response explicitly reports network_accessed=false. Secret references render as masked placeholders; preview never resolves them.

## 5. NATS event transport

NATS is private backend infrastructure. The browser never connects directly to NATS. Rust publishes versioned stats snapshots on subject automation.v1.<workspace_id>.runs.<run_id>.stats; the Rust SSE gateway subscribes only after API authorization. Messages include schema_version, run_id, sequence, occurred_at, and the sanitized stats payload. The NATS server enforces service credentials and subject allow-lists. SSE remains the browser-facing protocol, with a fresh SQLite snapshot on connect/reconnect.

## 6. Run comparison

GET /api/v1/runs/{run_id}/comparison?baseline=last_successful finds the latest earlier PASS for the same suite revision and environment. It returns baseline run ID/time, status changes by case/iteration/step, duration deltas, and API latency aggregate deltas. It omits payloads and sensitive row values. If no eligible baseline exists, return HTTP 200 with baseline=null and a clear reason.

## 7. Run event schema

Each event has event_id, run_id, sequence, event_type, occurred_at, and payload. Event types include run.queued, run.started, case.started, step.started, step.log, step.finished, run.progress, run.stats.updated, case.iteration.started, run.finished, run.interrupted, notification.failed.

The Rust gateway sends an initial full snapshot, then NATS-backed events ordered by sequence. The client can reconnect using Last-Event-ID; the server resumes from live events or returns a refreshed SQLite snapshot if that sequence has expired. Heartbeats keep the connection alive. Status transitions are immediate; aggregate statistics updates are coalesced to at most once per second. Event payloads are sanitized and size bounded.

The stats snapshot contains planned/terminal case iterations and nodes, counts by status, active and queued work, retry count, elapsed time, current case/step, and last_updated_at. If useful data exists, it may include API request count, status-class counts, p50/p95 response latency, and observed request rate. It never contains raw request bodies, credentials, secret values, or unbounded metric labels. The server returns progress_percentage only when the planned denominator is known; otherwise it returns exact counts and progress_mode=indeterminate. Any estimated remaining time is a separate field with estimate=true.

Example snapshot shape:

    {
      "run_id": "uuid",
      "status": "RUNNING",
      "progress": {"mode": "determinate", "percent": 42, "terminal_nodes": 42, "planned_nodes": 100},
      "cases": {"passed": 2, "failed": 0, "error": 0, "running": 1, "queued": 5},
      "nodes": {"passed": 38, "failed": 0, "running": 2, "queued": 55, "skipped": 4, "retry_wait": 1},
      "elapsed_ms": 184000,
      "estimated_remaining_ms": null,
      "estimate": false,
      "current": {"case_name": "Create customer", "step_name": "Wait for record", "attempt": 3},
      "api": {"requests": 18, "status_2xx": 16, "status_4xx": 1, "status_5xx": 1, "p50_ms": 82, "p95_ms": 241, "observed_requests_per_minute": 5.9},
      "last_updated_at": "2026-10-07T12:00:00Z"
    }

The example is illustrative; absent categories may be zero or omitted by schema policy. Counts are exact as of last_updated_at. The API latency values are run-level summaries, not load-test measurements. The rerun-failed endpoint creates a distinct run linked to the source run, reuses its failed case-iteration inputs and pinned revisions, and records which credential versions were resolved for the new run.

CI CLI behavior: the client calls the run trigger endpoint, waits using SSE or polling, downloads JUnit XML, and exits 0 only for PASS. FAIL, ERROR, CANCELED, or INTERRUPTED return non-zero exit status. The CLI never prints secret values and can write a machine-readable run summary to a caller-selected file.

## 8. Definition envelope

Every published asset includes:

    {
      "schema_version": 1,
      "asset_kind": "test_case",
      "metadata": {"name": "Create and verify customer"},
      "inputs": [{"name": "email", "type": "string", "required": true}],
      "variable_definitions": [],
      "nodes": [],
      "edges": [],
      "dependencies": [],
      "checksum": "sha256:..."
    }

The canvas position is presentation metadata. Runtime ordering and branching come from graph edges and explicit node semantics.

## 9. Errors

Use a stable problem response:

    {
      "type": "validation_error",
      "title": "Definition is invalid",
      "status": 422,
      "request_id": "uuid",
      "errors": [
        {"path": "nodes.4.config.timeout_seconds", "code": "out_of_range", "message": "Value exceeds workspace limit"}
      ]
    }

Stable error categories: unauthorized, forbidden, not_found, conflict, validation_error, rate_limited, capacity_exceeded, dependency_unavailable, import_rejected, internal_error. Do not return stack traces or secret-bearing values.

## 10. Compatibility and security

- Version the API and workflow schemas independently.
- Additive changes are preferred; breaking schema changes require migration support.
- Rate limit login, run triggers, connection tests, imports, exports, and webhook endpoints.
- Use secure, HttpOnly, SameSite cookies or short-lived bearer tokens for CI.
- CI tokens are scoped to suite/environment and can be revoked. Store only token hashes.
- The API never accepts credentials in workflow definitions; it accepts secret references managed through the connection UI.

