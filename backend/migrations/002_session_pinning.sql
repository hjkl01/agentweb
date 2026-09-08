ALTER TABLE sessions ADD COLUMN is_pinned INTEGER NOT NULL DEFAULT 0;
CREATE INDEX IF NOT EXISTS idx_sessions_pinned_updated ON sessions(is_pinned DESC, updated_at DESC);
