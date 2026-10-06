# Data Model and Import/Export

## 1. SQLite responsibilities

The local SQLite database is authoritative for users, suites, reusable assets, immutable revisions, connection metadata, run state, step results, audit events, schedules, and email outbox. The application stores larger logs/reports/artifacts in a local content-addressed directory and records checksums and metadata in SQLite.

SQLite runs on the same host/filesystem as the Rust service. Do not put the database on SMB/NFS or share it across hosts. Use WAL, foreign keys, bounded pools, short transactions, migrations, and a single run-event writer path.

## 2. Logical entities

- workspace: single workspace in the first release, with a schema that permits future workspaces.
- user, role_grant, session: identity and permission data.
- asset: reusable suite, case, node template, or script asset identity.
- asset_revision: immutable version, canonical definition JSON, checksum, author, timestamp, change note.
- asset_dependency: pinned revision references and aliases.
- environment: logical test environment.
- connection_profile: non-secret adapter settings and secret-reference IDs.
- variable_set/variable_definition: typed custom values and safe-function expressions scoped to environment, run, suite, case, or data iteration.
- function_catalog_version: version/hash of the built-in function schemas and evaluator used by a run.
- secret_record: encrypted payload, encryption-key version, secret version number, last rotation time.
- suite_run: run identity, initiating user, pinned suite revision, environment, status, timing, manifest, function-catalog version, random seed, and optional source_run_id for rerun/compare.
- case_run: case revision invocation and aggregate status.
- step_run: node invocation, attempt number, timing, status, sanitized output and artifact references.
- run_progress_snapshot: latest authorized-to-display case/node counters, current activity, timing aggregates, update timestamp, and event sequence; refreshed transactionally on state changes for fast UI/API reads.
- run_event: append-only lifecycle events for UI/report.
- run_progress_snapshot: latest progress counters and safe aggregate statistics for fast UI/API reads.
- nats_event_outbox: durable compact event records pending NATS publication, with sequence, retry count, and published timestamp.
- dataset_revision and case_iteration: immutable CSV/JSON input-set checksum/schema plus one reportable execution identity per row.
- resource_lock: environment/tenant/account lock owner, lease, heartbeat, and release/uncertain state.
- artifact: path, hash, MIME type, size, redaction status, retention time.
- audit_event: actor, action, target, timestamp, correlation ID, change summary.
- email_outbox: durable notification request and retry state.
- schedule/webhook: trigger configuration, secret/token hash, next-fire/status metadata.

Use UUIDs for externally visible IDs. Use foreign keys and uniqueness constraints on asset revision number and node ID within a revision. Definitions are canonical JSON with schema_version; use relational columns for queryable identity/state.

## 3. Suggested indexes and rules

- asset(workspace_id, kind, archived_at)
- asset_revision(asset_id, version DESC), UNIQUE(asset_id, version)
- asset_dependency(revision_id), asset_dependency(dependency_revision_id)
- suite_run(workspace_id, created_at DESC), suite_run(status, created_at)
- suite_run(suite_revision_id, environment_id, status, created_at DESC)
- case_run(suite_run_id, ordinal)
- step_run(case_run_id, ordinal, attempt)
- run_event(suite_run_id, sequence)
- resource_lock(resource_key, status, lease_expires_at)
- audit_event(created_at DESC, actor_id)
- email_outbox(status, next_attempt_at)
- nats_event_outbox(status, next_attempt_at)

Run status, step events, the latest progress snapshot, and NATS outbox records must be written in a transaction with a monotonic sequence per run. A finalized run is immutable except for notification delivery status and retention metadata. Dataset contents are stored as versioned assets or bounded artifacts; the run pins their hash and iteration count. Variable definitions and the random seed are pinned; secret values are never included in manifests or exports.

Run comparison is computed from retained summaries and step/API aggregates. The eligible baseline is the latest earlier PASS for the same suite revision and environment. The comparison view contains status and duration/latency deltas, not raw request/response data.

## 4. Portable bundle format

The UI exports a ZIP bundle with a manifest, a consistent SQLite snapshot or selected portable definitions, and optional approved assets. Default export is “definitions only”: suite/case revisions, dependency graph, typed non-secret variable definitions, non-secret connection shape, and script assets where the exporting user has permission.

    manifest.json
    database/automation-snapshot.sqlite
    assets/scripts/<sha256>
    artifacts/ (optional; admin only, explicit selection)
    checksums.sha256

Never export secret values, encryption keys, sessions, webhook signing secrets, or SMTP credentials. Secret references become unresolved placeholders requiring recipient re-entry. Built-in function names and catalog versions may export; resolved random values/seeds and run previews do not export with definitions. Run history and artifacts are opt-in and admin-only because reports can contain sensitive data.

The exporter creates a consistent SQLite snapshot using the supported SQLite backup mechanism, not a raw copy of an active WAL database. The manifest contains format version, minimum app version, creation timestamp, included asset IDs/revisions, and hashes.

## 5. Import procedure

1. Upload to a quarantine area; enforce archive size, expanded size, file count, and path traversal limits.
2. Validate ZIP structure, manifest version, hashes, SQLite integrity, schema migrations, JSON schemas, script asset policy, graph references, and dependency cycles.
3. Show preview: included suites/cases, revision conflicts, unresolved connections/secrets, asset sizes, and any unsupported node types.
4. Choose merge as new IDs, create separate workspace, or admin-only replace. Never silently overwrite.
5. Create a pre-import SQLite snapshot and write in one transaction.
6. Keep imported scripts disabled until re-reviewed and approved locally.
7. If any write fails, rollback and retain the pre-import restore point.
8. Record an audit event and show an import report.

Import rejects bundles with unknown schema versions, path traversal, invalid hashes, oversized assets, secrets, executable archive hooks, unsupported connector types, or dependency cycles.

## 6. Backup and restore

- UI backup downloads a consistent snapshot and optionally selected artifacts.
- Scheduled local backup creates timestamped encrypted snapshots and verifies integrity.
- Keep at least 7 daily and 4 weekly snapshots by default; administrators configure retention and off-host destination.
- Back up the secret encryption key separately with restricted access. A database restore without its matching key restores definitions and runs but cannot decrypt stored secrets.
- Document recovery point and recovery time targets. Proposed initial targets: RPO 24 hours for scheduled off-host backup; RTO 4 hours for a trained operator. These are operational goals, not guarantees.
- Restore is an explicit maintenance operation; validate the snapshot before replacing the live file.

