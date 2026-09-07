use anyhow::Result;
use sqlx::SqlitePool;

pub async fn init(pool: &SqlitePool) -> Result<()> {
    sqlx::query("CREATE TABLE IF NOT EXISTS agents (id TEXT PRIMARY KEY, name TEXT NOT NULL, kind TEXT NOT NULL, command TEXT NOT NULL, working_directory TEXT, enabled INTEGER NOT NULL DEFAULT 1, installed INTEGER NOT NULL DEFAULT 0, version TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL)").execute(pool).await?;
    sqlx::query("CREATE TABLE IF NOT EXISTS sessions (id TEXT PRIMARY KEY, agent_id TEXT NOT NULL, title TEXT NOT NULL, workspace TEXT NOT NULL, status TEXT NOT NULL, native_session_id TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL)").execute(pool).await?;
    let _ = sqlx::query("ALTER TABLE sessions ADD COLUMN native_session_id TEXT").execute(pool).await;
    sqlx::query("CREATE TABLE IF NOT EXISTS messages (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, role TEXT NOT NULL, content TEXT NOT NULL, created_at TEXT NOT NULL)").execute(pool).await?;
    sqlx::query("CREATE TABLE IF NOT EXISTS runtime_settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)").execute(pool).await?;
    Ok(())
}
