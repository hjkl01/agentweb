-- Durable scheduled tasks and execution history.
-- Timestamps are stored as UTC RFC3339 strings, consistent with the existing schema.

CREATE TABLE IF NOT EXISTS scheduled_tasks (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 120),
    prompt TEXT NOT NULL CHECK (length(trim(prompt)) > 0),
    agent_id TEXT NOT NULL,
    working_directory TEXT NOT NULL,
    model TEXT,
    schedule_type TEXT NOT NULL CHECK (schedule_type IN ('once', 'interval', 'cron')),
    schedule_config TEXT NOT NULL CHECK (json_valid(schedule_config)),
    timezone TEXT NOT NULL DEFAULT 'UTC',
    session_strategy TEXT NOT NULL DEFAULT 'new'
        CHECK (session_strategy IN ('new', 'reuse')),
    reuse_session_id TEXT,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    next_run_at TEXT,
    last_run_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK (
        (session_strategy = 'new' AND reuse_session_id IS NULL)
        OR
        (session_strategy = 'reuse' AND reuse_session_id IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_scheduled_tasks_due
    ON scheduled_tasks(enabled, next_run_at);

CREATE TABLE IF NOT EXISTS scheduled_task_runs (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    task_name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'queued'
        CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
    scheduled_at TEXT NOT NULL,
    started_at TEXT,
    finished_at TEXT,
    agent_session_id TEXT,
    summary TEXT,
    error_message TEXT,
    lease_owner TEXT,
    lease_until TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- A single scheduled occurrence can only produce one durable run, even when
-- multiple backend processes poll the same SQLite database concurrently.
CREATE UNIQUE INDEX IF NOT EXISTS idx_scheduled_task_runs_occurrence
    ON scheduled_task_runs(task_id, scheduled_at);

CREATE INDEX IF NOT EXISTS idx_scheduled_task_runs_queue
    ON scheduled_task_runs(status, lease_until, scheduled_at);

CREATE INDEX IF NOT EXISTS idx_scheduled_task_runs_task_history
    ON scheduled_task_runs(task_id, created_at DESC);
