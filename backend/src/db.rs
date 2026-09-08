use anyhow::Result;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use uuid::Uuid;

pub async fn init(pool: &SqlitePool) -> Result<()> {
    sqlx::query("CREATE TABLE IF NOT EXISTS agents (id TEXT PRIMARY KEY, name TEXT NOT NULL, kind TEXT NOT NULL, command TEXT NOT NULL, working_directory TEXT, enabled INTEGER NOT NULL DEFAULT 1, installed INTEGER NOT NULL DEFAULT 0, version TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL)").execute(pool).await?;
    sqlx::query("CREATE TABLE IF NOT EXISTS sessions (id TEXT PRIMARY KEY, agent_id TEXT NOT NULL, title TEXT NOT NULL, workspace TEXT NOT NULL, status TEXT NOT NULL, native_session_id TEXT, model TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL)").execute(pool).await?;
    let _ = sqlx::query("ALTER TABLE sessions ADD COLUMN native_session_id TEXT").execute(pool).await;
    let _ = sqlx::query("ALTER TABLE sessions ADD COLUMN model TEXT").execute(pool).await;
    sqlx::query("CREATE TABLE IF NOT EXISTS messages (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, role TEXT NOT NULL, content TEXT NOT NULL, created_at TEXT NOT NULL)").execute(pool).await?;
    sqlx::query("CREATE TABLE IF NOT EXISTS runtime_settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)").execute(pool).await?;
    sqlx::query("CREATE TABLE IF NOT EXISTS users (id TEXT PRIMARY KEY, username TEXT NOT NULL UNIQUE, password_hash TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL)").execute(pool).await?;
    seed_builtin_agents(pool).await?;
    Ok(())
}

async fn seed_builtin_agents(pool: &SqlitePool) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    let agents = [
        ("codex", "Codex", "codex", "codex"),
        ("pi", "Pi", "pi", "pi"),
        ("opencode", "OpenCode", "opencode", "opencode"),
        ("openclaw", "OpenClaw", "openclaw", "openclaw"),
        ("claude-code", "Claude Code", "claude-code", "claude"),
    ];
    for (id, name, kind, command) in agents {
        sqlx::query("INSERT OR IGNORE INTO agents(id,name,kind,command,installed,created_at,updated_at) VALUES(?,?,?,?,0,?,?)")
            .bind(id).bind(name).bind(kind).bind(command).bind(&now).bind(&now).execute(pool).await?;
    }
    Ok(())
}

/// Create the first admin account on a fresh database.
/// The generated password is only returned to the caller on first creation and is
/// never stored in plaintext; only its SHA-256 hash is persisted.
pub async fn ensure_default_admin(pool: &SqlitePool) -> Result<Option<String>> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(pool).await?;
    if count > 0 { return Ok(None); }
    let password = Uuid::new_v4().simple().to_string();
    let password_hash = format!("{:x}", Sha256::digest(password.as_bytes()));
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query("INSERT INTO users(id,username,password_hash,created_at,updated_at) VALUES(?,?,?,?,?)")
        .bind(Uuid::new_v4().to_string()).bind("admin").bind(password_hash).bind(&now).bind(&now).execute(pool).await?;
    Ok(Some(password))
}
