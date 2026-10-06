CREATE TABLE IF NOT EXISTS disabled_users (
    user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    disabled_at TEXT NOT NULL
);
