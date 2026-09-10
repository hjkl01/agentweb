-- Keep session-related data from becoming orphaned after a session is deleted.
CREATE TRIGGER IF NOT EXISTS trg_sessions_delete_messages
AFTER DELETE ON sessions
BEGIN
    DELETE FROM messages WHERE session_id = OLD.id;
END;

CREATE TRIGGER IF NOT EXISTS trg_sessions_delete_agent_events
AFTER DELETE ON sessions
BEGIN
    DELETE FROM agent_events WHERE session_id = OLD.id;
END;
