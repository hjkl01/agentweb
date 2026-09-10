CREATE TABLE IF NOT EXISTS login_failures (
    client_ip TEXT PRIMARY KEY,
    failures INTEGER NOT NULL DEFAULT 0,
    locked_until INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_login_failures_updated_at
    ON login_failures(updated_at);
