CREATE TABLE IF NOT EXISTS login_attempts (
    identity_key TEXT PRIMARY KEY,
    window_started_at TEXT NOT NULL,
    attempt_count INTEGER NOT NULL DEFAULT 0
);
