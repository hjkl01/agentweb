use anyhow::Result;
use argon2::password_hash::rand_core::OsRng;
use argon2::{
    password_hash::{PasswordHasher, SaltString},
    Argon2,
};
use sqlx::SqlitePool;
use uuid::Uuid;

pub async fn init(pool: &SqlitePool) -> Result<()> {
    sqlx::migrate!("./migrations").run(pool).await?;
    // Conversation transcripts are owned by Agent-native session files.
    // Drop the legacy table after migrations so existing databases stop retaining
    // message bodies. Keep the historical migration unchanged for checksum safety.
    sqlx::query("DROP TABLE IF EXISTS messages").execute(pool).await?;
    sqlx::query("DELETE FROM agent_events WHERE session_id IS NOT NULL")
        .execute(pool)
        .await?;

    // Separate ephemeral execution coordination from durable conversation
    // metadata. This compatibility layer is intentionally metadata-free: it
    // stores only ownership/lease state, never transcript text or titles.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS session_runtime (
            session_id TEXT PRIMARY KEY,
            status TEXT NOT NULL DEFAULT 'idle',
            worker_id TEXT,
            lease_until TEXT,
            updated_at TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT OR IGNORE INTO session_runtime(session_id,status,worker_id,lease_until,updated_at)
         SELECT id,status,worker_id,lease_until,updated_at FROM sessions",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TRIGGER IF NOT EXISTS sessions_runtime_insert AFTER INSERT ON sessions
         BEGIN
           INSERT INTO session_runtime(session_id,status,worker_id,lease_until,updated_at)
           VALUES(NEW.id,NEW.status,NEW.worker_id,NEW.lease_until,NEW.updated_at)
           ON CONFLICT(session_id) DO UPDATE SET status=excluded.status,
             worker_id=excluded.worker_id, lease_until=excluded.lease_until,
             updated_at=excluded.updated_at;
         END",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TRIGGER IF NOT EXISTS sessions_runtime_update
         AFTER UPDATE OF status,worker_id,lease_until,updated_at ON sessions
         BEGIN
           INSERT INTO session_runtime(session_id,status,worker_id,lease_until,updated_at)
           VALUES(NEW.id,NEW.status,NEW.worker_id,NEW.lease_until,NEW.updated_at)
           ON CONFLICT(session_id) DO UPDATE SET status=excluded.status,
             worker_id=excluded.worker_id, lease_until=excluded.lease_until,
             updated_at=excluded.updated_at;
         END",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TRIGGER IF NOT EXISTS sessions_runtime_delete AFTER DELETE ON sessions
         BEGIN DELETE FROM session_runtime WHERE session_id=OLD.id; END",
    )
    .execute(pool)
    .await?;
    // Keep legacy API readers compatible while lifecycle writes are migrated
    // incrementally to session_runtime. The WHEN guard prevents needless
    // trigger ping-pong when the mirrored values already match.
    sqlx::query(
        "CREATE TRIGGER IF NOT EXISTS runtime_sessions_update
         AFTER UPDATE OF status,worker_id,lease_until,updated_at ON session_runtime
         WHEN EXISTS (
           SELECT 1 FROM sessions s
           WHERE s.id=NEW.session_id AND
             (s.status != NEW.status OR
              COALESCE(s.worker_id,'') != COALESCE(NEW.worker_id,'') OR
              COALESCE(s.lease_until,'') != COALESCE(NEW.lease_until,'') OR
              s.updated_at != NEW.updated_at)
         )
         BEGIN
           UPDATE sessions SET status=NEW.status,worker_id=NEW.worker_id,
             lease_until=NEW.lease_until,updated_at=NEW.updated_at
           WHERE id=NEW.session_id;
         END",
    )
    .execute(pool)
    .await?;
    // Remove stale coordination rows left by sessions deleted before the
    // compatibility trigger existed. Runtime records never define history.
    sqlx::query(
        "DELETE FROM session_runtime WHERE NOT EXISTS
         (SELECT 1 FROM sessions WHERE sessions.id=session_runtime.session_id)",
    )
    .execute(pool)
    .await?;
    seed_builtin_agents(pool).await?;
    Ok(())
}

async fn seed_builtin_agents(pool: &SqlitePool) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    let agents = [
        ("codex", "Codex", "codex", "codex"),
        ("claude-code", "Claude Code", "claude-code", "claude"),
        ("gemini-cli", "Gemini CLI", "gemini-cli", "gemini"),
        ("opencode", "OpenCode", "opencode", "opencode"),
        ("pi", "Pi", "pi", "pi"),
        ("qwen-code", "Qwen Code", "qwen-code", "qwen"),
    ];
    for (id, name, kind, command) in agents {
        sqlx::query("INSERT OR IGNORE INTO agents(id,name,kind,command,installed,created_at,updated_at) VALUES(?,?,?,?,0,?,?)").bind(id).bind(name).bind(kind).bind(command).bind(&now).bind(&now).execute(pool).await?;
    }
    sqlx::query("DELETE FROM agents WHERE id='openclaw'")
        .execute(pool)
        .await?;
    Ok(())
}

fn initial_password() -> String {
    const UPPER: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    const LOWER: &[u8] = b"abcdefghijklmnopqrstuvwxyz";
    const DIGIT: &[u8] = b"0123456789";
    const SPECIAL: &[u8] = b"!@#$%^&*_-+=?";
    const ALL: &[u8] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*_-+=?";
    let a = *Uuid::new_v4().as_bytes();
    let b = *Uuid::new_v4().as_bytes();
    let pick = |set: &[u8], byte: u8| set[byte as usize % set.len()] as char;
    let mut password = vec![
        pick(UPPER, a[0]),
        pick(LOWER, a[1]),
        pick(DIGIT, a[2]),
        pick(SPECIAL, a[3]),
    ];
    for index in 0..8 {
        password.push(pick(ALL, b[index]));
    }
    for index in (1..password.len()).rev() {
        let swap = b[(index + 4) % b.len()] as usize % (index + 1);
        password.swap(index, swap);
    }
    password.into_iter().collect()
}

pub async fn ensure_default_admin(pool: &SqlitePool) -> Result<Option<String>> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?;
    if count > 0 {
        return Ok(None);
    };
    let password = initial_password();
    let salt = SaltString::generate(&mut OsRng);
    let password_hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|error| anyhow::anyhow!("default admin password hashing failed: {error}"))?
        .to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let result=sqlx::query("INSERT OR IGNORE INTO users(id,username,password_hash,created_at,updated_at) VALUES(?,?,?,?,?)").bind(Uuid::new_v4().to_string()).bind("admin").bind(password_hash).bind(&now).bind(&now).execute(pool).await?;
    if result.rows_affected() == 1 {
        Ok(Some(password))
    } else {
        Ok(None)
    }
}
