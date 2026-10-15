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
    // Remove the legacy cleanup trigger before dropping its target table; otherwise
    // deleting a session can fail when SQLite executes a trigger that references
    // the removed messages table. Keep historical migrations unchanged for checksum safety.
    sqlx::query("DROP TRIGGER IF EXISTS trg_sessions_delete_messages")
        .execute(pool)
        .await?;
    sqlx::query("DROP TABLE IF EXISTS messages").execute(pool).await?;
    sqlx::query("DELETE FROM agent_events WHERE session_id IS NOT NULL")
        .execute(pool)
        .await?;

    // Remove legacy bidirectional synchronization. Runtime state is now
    // written explicitly by session lifecycle operations; keeping these
    // triggers would reintroduce the old sessions/runtime coupling.
    sqlx::query("DROP TRIGGER IF EXISTS sessions_runtime_insert")
        .execute(pool)
        .await?;
    sqlx::query("DROP TRIGGER IF EXISTS sessions_runtime_update")
        .execute(pool)
        .await?;
    sqlx::query("DROP TRIGGER IF EXISTS sessions_runtime_delete")
        .execute(pool)
        .await?;
    sqlx::query("DROP TRIGGER IF EXISTS runtime_sessions_update")
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

#[cfg(test)]
mod tests {
    use super::init;
    use sqlx::sqlite::SqlitePoolOptions;

    #[tokio::test]
    async fn init_removes_legacy_session_runtime_triggers() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();

        init(&pool).await.unwrap();

        for name in [
            "sessions_runtime_insert",
            "sessions_runtime_update",
            "sessions_runtime_delete",
            "runtime_sessions_update",
        ] {
            let sql = format!(
                "CREATE TRIGGER {} AFTER INSERT ON sessions BEGIN SELECT 1; END",
                name
            );
            sqlx::query(&sql).execute(&pool).await.unwrap();
        }

        // Running initialization against an existing database must remove
        // legacy triggers as well as avoid recreating them.
        init(&pool).await.unwrap();

        let remaining: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='trigger' AND name IN ('sessions_runtime_insert','sessions_runtime_update','sessions_runtime_delete','runtime_sessions_update')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(remaining, 0, "legacy runtime synchronization triggers must be absent");
    }
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
