ALTER TABLE sessions ADD COLUMN worker_id TEXT;
ALTER TABLE sessions ADD COLUMN lease_until TEXT;

CREATE INDEX IF NOT EXISTS idx_sessions_running_lease ON sessions(status, lease_until);
