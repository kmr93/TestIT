# Security, Reporting, and Operations

## 1. Threat model

The highest-risk inputs are executable shell/Python assets, imported bundles, user-supplied URLs/queries, database credentials, and report contents that may contain production data. Treat all of them as untrusted. The platform should assume a test can accidentally reach the wrong environment unless profiles and egress are explicit.

## 2. Identity and authorization

- Multi-user roles: Admin, Author, Runner, Viewer.
- Enforce workspace and environment permissions in every API handler and worker-launch path.
- Local password accounts use a current memory-hard password hash, secure session cookies, rate-limited login, CSRF protection, session expiry, and revocation.
- Store audit events for login/admin changes, secret updates/access, revision publication, imports/exports, run triggers/cancellations, and permission changes.
- OIDC SSO is a planned integration; use a standards-based provider instead of inventing SSO.

## 3. Secret handling

- Encrypt credential values with authenticated encryption and an envelope-key design. The master key lives outside SQLite in an OS-protected secret file or external key service.
- Keep only references and encrypted payload in SQLite. Never store plaintext in browser state, logs, reports, imports, or email.
- Inject credentials only for the step that declares them. Prefer private files/FDs over command-line arguments; avoid environment variables where process inspection could expose them.
- Redact authorization headers, passwords, tokens, connection strings, cookies, and configured sensitive output paths.
- Record secret ID/version use, not values. Restrict “test connection” to authorized users.
- Provide key rotation and backup/restore guidance. Losing the key makes encrypted credentials unrecoverable.
- Treat SecretRef variables as tainted values through request construction and outputs. Variable preview uses sample values and masked secret placeholders; it must never decrypt a target credential solely for preview.

## 4. Worker isolation and network policy

- Use short-lived, non-root workers with read-only root filesystem, no-new-privileges, dropped capabilities, no privileged mode, no Docker socket, bounded CPU/memory/pids, deadline, output cap, and dedicated temporary directory.
- Mount only the selected script and node input read-only; mount one empty output directory with a quota.
- Do not install packages during a run. Build and scan pinned worker images.
- Default network policy is deny. Administrators allow target hosts/ports by environment; block cloud metadata, loopback, and unapproved destinations. Resolve and validate DNS at connection time to reduce rebinding risk.
- Permit private networks only when an administrator explicitly adds the intended target. Backend test systems commonly live on private networks, so a blanket private-IP ban would make the product unusable.
- If the container engine or configured egress control is unavailable, disable execution. Never run a script directly on the application host.

Docker is a container boundary, not a complete defense against every hostile workload. Keep the host patched, use a supported Linux deployment, restrict who can publish scripts, and evaluate a stronger sandbox for untrusted multi-tenant workloads.

## 5. Database and query safety

- Require read-only database accounts for validation nodes.
- Use parameterized queries and bounded result sizes.
- Separate destructive setup/cleanup credentials and nodes. They require explicit admin policy and are disabled by default.
- Store TLS verification enabled by default; insecure TLS override is admin-only and audited.
- Log query fingerprint and safe parameters only, never full sensitive row contents.
- Apply per-connection timeouts and server-side limits where available.

## 6. Import/export safety

- Treat ZIP contents as untrusted. Reject path traversal, symlinks, oversized expansion, unknown schemas, invalid checksums, and unauthorized scripts.
- Parse assets in quarantine; do not execute scripts as part of validation.
- Treat OpenAPI import as untrusted input: limit document size and reference depth, reject local-file references, and fetch remote references only from administrator-approved origins through the same egress controls as workers.
- Default exports omit run artifacts and secrets. Rebind credentials on import.
- Require admin approval before imported script revisions can run.

## 7. Reports and email

Statuses: PASS, FAIL (assertion or expected result mismatch), ERROR (adapter, infrastructure, or worker failure), CANCELED, INTERRUPTED/UNKNOWN.

Reports include revision IDs, environment label, step sequence, timing, assertions, sanitized error, log snippets, and artifact links. HTML output escapes all user-controlled content and uses a restrictive content security policy. Artifacts require authorization and short-lived downloads.

Live run statistics are derived from durable status/timing events. Expose counts and aggregate latencies only; do not place sensitive request/response data in SSE snapshots. Apply the same object-level authorization to live subscriptions as to reports. On reconnect, re-check authorization and return a fresh sanitized snapshot.

NATS is reachable only on the private Compose network and has no published host port. Use dedicated service credentials and allow-list the required stats subjects; subject-level permissions are NATS's authorization boundary. See [NATS authorization](https://docs.nats.io/learn/security/authorization). Use TLS for client connections when traffic crosses a host boundary or an untrusted network; see [NATS TLS](https://docs.nats.io/learn/security/encryption). The browser receives only the Rust service's authenticated SSE stream and never receives NATS credentials. Validate message schema/version, run ID, sequence, and size before forwarding. Do not put secrets or request bodies in NATS payloads.

If NATS disconnects, preserve run state in SQLite, expose the current snapshot over the stats API, and switch the UI to bounded polling with a degraded/stale indicator. When NATS reconnects, refresh active subscriptions from SQLite before resuming live fan-out. Core messages are ephemeral, so the broker is not the historical record.

Variable preview is a no-network operation using explicit sample inputs. It must not invoke Playwright, a script worker, database adapter, or environment secret. Run comparison includes only statuses, timings, and aggregate API statistics; it never returns record-level payload differences.

SMTP is configured by an Admin using server, port, TLS mode, auth reference, sender, and test recipient. Verify TLS certificates. Store SMTP credentials as encrypted secrets. Use a transactional outbox so a temporary mail outage does not lose the completion notification. Email includes a concise status summary and authenticated report link; no SQLite or raw logs attached.

## 8. Retention and backups

Defaults: run summaries 90 days, detailed logs/artifacts 30 days, artifact quota 20 GiB, configurations retained until archived/deleted, audit retention 1 year. Admin may change policy subject to disk capacity. Before deleting artifacts, update metadata and preserve the run summary.

Nightly consistent snapshot to a restricted local backup folder; recommend an off-host encrypted copy. Verify backups periodically. Keep the secret key backup separately secured. A backup job must report failure visibly; “backup configured” is not proof that restore works.

## 9. Operational controls

- Deploy behind a TLS-terminating reverse proxy; bind the application to a private interface or localhost.
- One Docker Compose host with a pinned NATS Core service on a private network, persistent volume for SQLite and artifact directory, separate worker image, and no public exposure of Docker or NATS ports.
- Readiness blocks triggers when storage or worker execution is unavailable.
- Report NATS connectivity as a degraded real-time channel; durable suite execution/report state remains available through SQLite, and the UI falls back to polling.
- Metrics: queue length, worker count, duration, status, crash/timeout, DB busy, storage quota, email outbox age.
- Live run dashboard: status counters, active node, iteration and node progress, elapsed time, retries, API status/latency aggregates, stream health, and a visible stale indicator after disconnect.
- Alert on repeated worker crashes, out-of-disk threshold, failed backups, stale queue, and mail backlog.
- Upgrade procedure: backup, stop writes, apply forward-only migrations, restart, validate readiness. Keep rollback instructions.
- Incident procedure: disable triggers, stop workers, preserve database/artifacts, rotate affected credentials, inspect audit events, restore only from validated backup.

