-- TestIT Initial SQLite Migration
-- Enforces WAL mode, foreign keys, and indexes for reliable, transactional execution.

PRAGMA foreign_keys = ON;

-- 1. Workspace
CREATE TABLE IF NOT EXISTS workspaces (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- 2. Users & Sessions
CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    email TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('ADMIN', 'AUTHOR', 'RUNNER', 'VIEWER')),
    password_hash TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL UNIQUE,
    expires_at TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- 3. Assets & Revisions
CREATE TABLE IF NOT EXISTS assets (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('suite', 'case', 'template', 'script')),
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    draft_json TEXT NOT NULL DEFAULT '{}',
    draft_version INTEGER NOT NULL DEFAULT 1,
    archived_at TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_assets_workspace_kind ON assets(workspace_id, kind, archived_at);

CREATE TABLE IF NOT EXISTS asset_revisions (
    id TEXT PRIMARY KEY,
    asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    version INTEGER NOT NULL,
    definition_json TEXT NOT NULL,
    checksum TEXT NOT NULL,
    change_note TEXT NOT NULL DEFAULT '',
    author_id TEXT REFERENCES users(id),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE(asset_id, version)
);
CREATE INDEX IF NOT EXISTS idx_asset_revisions_lookup ON asset_revisions(asset_id, version DESC);

CREATE TABLE IF NOT EXISTS asset_dependencies (
    revision_id TEXT NOT NULL REFERENCES asset_revisions(id) ON DELETE CASCADE,
    dependency_revision_id TEXT NOT NULL REFERENCES asset_revisions(id) ON DELETE RESTRICT,
    alias TEXT NOT NULL,
    PRIMARY KEY (revision_id, dependency_revision_id)
);
CREATE INDEX IF NOT EXISTS idx_asset_dep_rev ON asset_dependencies(revision_id);
CREATE INDEX IF NOT EXISTS idx_asset_dep_target ON asset_dependencies(dependency_revision_id);

-- 4. Environments & Connections & Secrets
CREATE TABLE IF NOT EXISTS environments (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    variables_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS connection_profiles (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    connector_type TEXT NOT NULL,
    settings_json TEXT NOT NULL DEFAULT '{}',
    secret_refs_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS secrets (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name TEXT NOT NULL UNIQUE,
    encrypted_payload TEXT NOT NULL,
    key_version INTEGER NOT NULL DEFAULT 1,
    secret_version INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- 5. Runs & Execution State
CREATE TABLE IF NOT EXISTS suite_runs (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    suite_revision_id TEXT NOT NULL REFERENCES asset_revisions(id),
    environment_id TEXT NOT NULL REFERENCES environments(id),
    status TEXT NOT NULL CHECK (status IN ('QUEUED', 'RUNNING', 'PASSED', 'FAILED', 'ERROR', 'CANCELED', 'INTERRUPTED')),
    initiating_user_id TEXT REFERENCES users(id),
    idempotency_key TEXT UNIQUE,
    run_manifest_json TEXT NOT NULL,
    random_seed INTEGER NOT NULL,
    catalog_version TEXT NOT NULL DEFAULT 'v1',
    inputs_json TEXT NOT NULL DEFAULT '{}',
    variable_overrides_json TEXT NOT NULL DEFAULT '{}',
    source_run_id TEXT REFERENCES suite_runs(id),
    started_at TEXT,
    finished_at TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_suite_runs_workspace ON suite_runs(workspace_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_suite_runs_status ON suite_runs(status, created_at);
CREATE INDEX IF NOT EXISTS idx_suite_runs_comp ON suite_runs(suite_revision_id, environment_id, status, created_at DESC);

CREATE TABLE IF NOT EXISTS case_runs (
    id TEXT PRIMARY KEY,
    suite_run_id TEXT NOT NULL REFERENCES suite_runs(id) ON DELETE CASCADE,
    case_revision_id TEXT NOT NULL REFERENCES asset_revisions(id),
    ordinal INTEGER NOT NULL,
    iteration_index INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL CHECK (status IN ('PENDING', 'RUNNING', 'PASSED', 'FAILED', 'ERROR', 'SKIPPED', 'CANCELED')),
    inputs_json TEXT NOT NULL DEFAULT '{}',
    started_at TEXT,
    finished_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_case_runs_suite ON case_runs(suite_run_id, ordinal);

CREATE TABLE IF NOT EXISTS step_runs (
    id TEXT PRIMARY KEY,
    case_run_id TEXT NOT NULL REFERENCES case_runs(id) ON DELETE CASCADE,
    node_id TEXT NOT NULL,
    node_name TEXT NOT NULL,
    node_type TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    attempt INTEGER NOT NULL DEFAULT 1,
    status TEXT NOT NULL CHECK (status IN ('PENDING', 'QUEUED', 'RUNNING', 'RETRY_WAIT', 'SUCCEEDED', 'ASSERTION_FAILED', 'ERROR', 'TIMED_OUT', 'CANCELED', 'SKIPPED', 'INTERRUPTED')),
    duration_ms REAL,
    error_json TEXT,
    outputs_json TEXT,
    metrics_json TEXT,
    started_at TEXT,
    finished_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_step_runs_case ON step_runs(case_run_id, ordinal, attempt);

-- 6. Live Snapshots, Events & Outbox
CREATE TABLE IF NOT EXISTS run_progress_snapshots (
    suite_run_id TEXT PRIMARY KEY REFERENCES suite_runs(id) ON DELETE CASCADE,
    status TEXT NOT NULL,
    progress_json TEXT NOT NULL,
    stats_json TEXT NOT NULL,
    last_sequence INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS run_events (
    id TEXT PRIMARY KEY,
    suite_run_id TEXT NOT NULL REFERENCES suite_runs(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL,
    event_type TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    occurred_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_run_events_seq ON run_events(suite_run_id, sequence);

CREATE TABLE IF NOT EXISTS nats_event_outbox (
    id TEXT PRIMARY KEY,
    suite_run_id TEXT NOT NULL REFERENCES suite_runs(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL,
    subject TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('PENDING', 'PUBLISHED', 'FAILED')),
    attempts INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_nats_outbox_queue ON nats_event_outbox(status, next_attempt_at);

-- 7. Resource Locks, Artifacts, Auditing & Email Outbox
CREATE TABLE IF NOT EXISTS resource_locks (
    resource_key TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES suite_runs(id) ON DELETE CASCADE,
    owner TEXT NOT NULL,
    lease_expires_at TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('HELD', 'EXPIRED', 'UNCERTAIN'))
);
CREATE INDEX IF NOT EXISTS idx_resource_locks ON resource_locks(resource_key, status, lease_expires_at);

CREATE TABLE IF NOT EXISTS artifacts (
    id TEXT PRIMARY KEY,
    suite_run_id TEXT NOT NULL REFERENCES suite_runs(id) ON DELETE CASCADE,
    step_run_id TEXT REFERENCES step_runs(id) ON DELETE SET NULL,
    name TEXT NOT NULL,
    path TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    checksum TEXT NOT NULL,
    retention_expires_at TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS audit_events (
    id TEXT PRIMARY KEY,
    actor_id TEXT REFERENCES users(id),
    action TEXT NOT NULL,
    target_type TEXT NOT NULL,
    target_id TEXT NOT NULL,
    correlation_id TEXT,
    changes_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_audit_created ON audit_events(created_at DESC, actor_id);

CREATE TABLE IF NOT EXISTS email_outbox (
    id TEXT PRIMARY KEY,
    suite_run_id TEXT REFERENCES suite_runs(id) ON DELETE CASCADE,
    recipient TEXT NOT NULL,
    subject TEXT NOT NULL,
    body_html TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('PENDING', 'SENT', 'FAILED')),
    attempts INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_email_outbox ON email_outbox(status, next_attempt_at);
