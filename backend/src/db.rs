use anyhow::Result;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use uuid::Uuid;

pub async fn init(pool:&SqlitePool)->Result<()>{
    sqlx::migrate!("./migrations").run(pool).await?;
    seed_builtin_agents(pool).await?;
    Ok(())
}

async fn seed_builtin_agents(pool:&SqlitePool)->Result<()>{
    let now=chrono::Utc::now().to_rfc3339();
    let agents=[
        ("codex","Codex","codex","codex"),
        ("claude-code","Claude Code","claude-code","claude"),
        ("qwen-code","Qwen Code","qwen-code","qwen"),
        ("gemini-cli","Gemini CLI","gemini-cli","gemini"),
        ("pi","Pi","pi","pi"),
        ("opencode","OpenCode","opencode","opencode"),
        ("openclaw","OpenClaw","openclaw","openclaw"),
    ];
    for (id,name,kind,command) in agents {
        sqlx::query("INSERT OR IGNORE INTO agents(id,name,kind,command,installed,created_at,updated_at) VALUES(?,?,?,?,0,?,?)").bind(id).bind(name).bind(kind).bind(command).bind(&now).bind(&now).execute(pool).await?;
    }
    Ok(())
}

fn initial_password()->String{
    const UPPER:&[u8]=b"ABCDEFGHIJKLMNOPQRSTUVWXYZ"; const LOWER:&[u8]=b"abcdefghijklmnopqrstuvwxyz"; const DIGIT:&[u8]=b"0123456789"; const SPECIAL:&[u8]=b"!@#$%^&*_-+=?"; const ALL:&[u8]=b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*_-+=?";
    let a=*Uuid::new_v4().as_bytes(); let b=*Uuid::new_v4().as_bytes(); let pick=|set:&[u8],byte:u8|set[byte as usize%set.len()] as char;
    let mut password=vec![pick(UPPER,a[0]),pick(LOWER,a[1]),pick(DIGIT,a[2]),pick(SPECIAL,a[3])]; for index in 0..8{password.push(pick(ALL,b[index]));}
    for index in (1..password.len()).rev(){let swap=b[(index+4)%b.len()] as usize%(index+1);password.swap(index,swap);} password.into_iter().collect()
}

pub async fn ensure_default_admin(pool:&SqlitePool)->Result<Option<String>>{
    let count:i64=sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(pool).await?; if count>0{return Ok(None)};
    let password=initial_password(); let password_hash=format!("{:x}",Sha256::digest(password.as_bytes())); let now=chrono::Utc::now().to_rfc3339();
    sqlx::query("INSERT INTO users(id,username,password_hash,created_at,updated_at) VALUES(?,?,?,?,?)").bind(Uuid::new_v4().to_string()).bind("admin").bind(password_hash).bind(&now).bind(&now).execute(pool).await?; Ok(Some(password))
}
