use anyhow::Result;
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

async fn column_exists(pool: &SqlitePool, table: &str, column: &str) -> Result<bool> {
    let rows = sqlx::query(&format!("PRAGMA table_info({table})")).fetch_all(pool).await?;
    Ok(rows.iter().any(|row| row.get::<String, _>(1) == column))
}

async fn migrate_schema(pool: &SqlitePool) -> Result<()> {
    sqlx::query("CREATE TABLE IF NOT EXISTS schema_meta (version INTEGER NOT NULL)").execute(pool).await?;
    let version: Option<i64> = sqlx::query_scalar("SELECT version FROM schema_meta LIMIT 1").fetch_optional(pool).await?;
    if version.is_none() { sqlx::query("INSERT INTO schema_meta(version) VALUES(1)").execute(pool).await?; }
    if !column_exists(pool, "sessions", "native_session_id").await? { sqlx::query("ALTER TABLE sessions ADD COLUMN native_session_id TEXT").execute(pool).await?; }
    if !column_exists(pool, "sessions", "model").await? { sqlx::query("ALTER TABLE sessions ADD COLUMN model TEXT").execute(pool).await?; }
    sqlx::query("CREATE TABLE IF NOT EXISTS auth_sessions (id TEXT PRIMARY KEY, user_id TEXT NOT NULL, token_hash TEXT NOT NULL UNIQUE, expires_at TEXT NOT NULL, created_at TEXT NOT NULL)").execute(pool).await?;
    sqlx::query("UPDATE schema_meta SET version=3").execute(pool).await?;
    Ok(())
}

pub async fn init(pool: &SqlitePool) -> Result<()> {
    sqlx::query("CREATE TABLE IF NOT EXISTS agents (id TEXT PRIMARY KEY, name TEXT NOT NULL, kind TEXT NOT NULL, command TEXT NOT NULL, working_directory TEXT, enabled INTEGER NOT NULL DEFAULT 1, installed INTEGER NOT NULL DEFAULT 0, version TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL)").execute(pool).await?;
    sqlx::query("CREATE TABLE IF NOT EXISTS sessions (id TEXT PRIMARY KEY, agent_id TEXT NOT NULL, title TEXT NOT NULL, workspace TEXT NOT NULL, status TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL)").execute(pool).await?;
    sqlx::query("CREATE TABLE IF NOT EXISTS messages (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, role TEXT NOT NULL, content TEXT NOT NULL, created_at TEXT NOT NULL)").execute(pool).await?;
    sqlx::query("CREATE TABLE IF NOT EXISTS runtime_settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)").execute(pool).await?;
    sqlx::query("CREATE TABLE IF NOT EXISTS users (id TEXT PRIMARY KEY, username TEXT NOT NULL UNIQUE, password_hash TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL)").execute(pool).await?;
    migrate_schema(pool).await?;
    seed_builtin_agents(pool).await?;
    Ok(())
}

async fn seed_builtin_agents(pool: &SqlitePool) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    let agents = [("codex", "Codex", "codex", "codex"), ("pi", "Pi", "pi", "pi"), ("opencode", "OpenCode", "opencode", "opencode"), ("openclaw", "OpenClaw", "openclaw", "openclaw"), ("claude-code", "Claude Code", "claude-code", "claude")];
    for (id, name, kind, command) in agents {
        sqlx::query("INSERT OR IGNORE INTO agents(id,name,kind,command,installed,created_at,updated_at) VALUES(?,?,?,?,0,?,?)").bind(id).bind(name).bind(kind).bind(command).bind(&now).bind(&now).execute(pool).await?;
    }
    Ok(())
}

fn initial_password() -> String {
    const UPPER: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    const LOWER: &[u8] = b"abcdefghijklmnopqrstuvwxyz";
    const DIGIT: &[u8] = b"0123456789";
    const SPECIAL: &[u8] = b"!@#$%^&*_-+=?";
    const ALL: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*_-+=?";
    let bytes = *Uuid::new_v4().as_bytes();
    let pick = |set: &[u8], index: usize| -> char { set[bytes[index] as usize % set.len()] as char };
    let mut password = vec![pick(UPPER, 0), pick(LOWER, 1), pick(DIGIT, 2), pick(SPECIAL, 3)];
    for index in 4..12 { password.push(pick(ALL, index)); }
    password.into_iter().collect()
}

/// Create the first admin account on a fresh database.
pub async fn ensure_default_admin(pool: &SqlitePool) -> Result<Option<String>> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(pool).await?;
    if count > 0 { return Ok(None); }
    let password = initial_password();
    let password_hash = format!("{:x}", Sha256::digest(password.as_bytes()));
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query("INSERT INTO users(id,username,password_hash,created_at,updated_at) VALUES(?,?,?,?,?)").bind(Uuid::new_v4().to_string()).bind("admin").bind(password_hash).bind(&now).bind(&now).execute(pool).await?;
    Ok(Some(password))
}
