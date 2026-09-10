ALTER TABLE agent_events ADD COLUMN worker_id TEXT;

CREATE INDEX IF NOT EXISTS idx_agent_events_session_worker_id
    ON agent_events(session_id, worker_id, id);
