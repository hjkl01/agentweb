use crate::{agents::AgentConfig, events::AgentEvent, installation::runtime, state::AppState};
use axum::extract::ws::{Message, WebSocket};
use axum::{
    extract::{Path, State, WebSocketUpgrade},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::{
    collections::HashMap,
    path::{Path as FsPath, PathBuf},
    process::Stdio,
};
use tokio::{fs, process::Command};
use uuid::Uuid;

#[derive(Serialize)]
pub struct Health {
    pub status: &'static str,
}
pub async fn health() -> Json<Health> {
    Json(Health { status: "ok" })
}
#[derive(Serialize)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub command: String,
    pub working_directory: Option<String>,
    pub installed: bool,
    pub version: Option<String>,
}
#[derive(Serialize)]
pub struct CatalogItem {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub installed: bool,
    pub requirements: Vec<&'static str>,
}

async fn setting(db: &sqlx::SqlitePool, key: &str) -> Option<String> {
    sqlx::query("SELECT value FROM runtime_settings WHERE key=?")
        .bind(key)
        .fetch_optional(db)
        .await
        .ok()
        .flatten()
        .map(|r| r.get(0))
}

async fn save_setting(db: &sqlx::SqlitePool, key: &str, value: &str) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO runtime_settings(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value")
        .bind(key)
        .bind(value)
        .execute(db)
        .await
        .map(|_| ())
}

async fn configured_agent_path(db: &sqlx::SqlitePool, id: &str) -> Option<String> {
    let raw = setting(db, "agent_paths").await?;
    let paths: HashMap<String, String> = serde_json::from_str(&raw).ok()?;
    paths.get(id).cloned().filter(|p| !p.trim().is_empty())
}

async fn configured_node_path(db: &sqlx::SqlitePool) -> Option<PathBuf> {
    setting(db, "node_path").await.filter(|p| !p.trim().is_empty()).map(PathBuf::from)
}

async fn node_bin_dir(db: &sqlx::SqlitePool) -> Option<PathBuf> {
    if let Some(path) = configured_node_path(db).await {
        if path.is_file() {
            return path.parent().map(PathBuf::from);
        }
        if path.join("node").is_file() {
            return Some(path);
        }
    }
    runtime::detect_installed_node()
        .await
        .ok()
        .flatten()
        .map(|(v, _)| runtime::node_bin(&v))
}

fn runtime_agent_installed(version: &str, id: &str) -> bool {
    runtime::node_bin(version).join(id).exists()
}

pub async fn catalog(State(s): State<AppState>) -> Json<Vec<CatalogItem>> {
    let codex_path = configured_agent_path(&s.db, "codex").await;
    let codex = if let Some(path) = codex_path {
        FsPath::new(&path).exists()
    } else {
        Command::new("codex").arg("--version").output().await.is_ok()
    };
    let node = node_bin_dir(&s.db).await;
    let node_version = if let Some(bin) = &node {
        let mut c = Command::new(bin.join("node"));
        c.arg("--version").output().await.ok().and_then(|o| {
            o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().trim_start_matches('v').to_owned())
        })
    } else {
        None
    };
    let configured = |id: &str| configured_agent_path(&s.db, id);
    let installed = |path: Option<String>, fallback: bool| async move {
        path.map(|p| FsPath::new(&p).exists()).unwrap_or(fallback)
    };
    Json(vec![
        CatalogItem { id: "codex", name: "Codex", description: "OpenAI coding agent", installed: codex, requirements: vec![] },
        CatalogItem { id: "claude-code", name: "Claude Code", description: "Anthropic coding agent", installed: installed(configured("claude-code").await, node.as_ref().map(|b| b.join("claude").exists()).unwrap_or(false)).await, requirements: vec!["Node.js"] },
        CatalogItem { id: "opencode", name: "OpenCode", description: "Open-source coding agent", installed: installed(configured("opencode").await, node.as_ref().map(|b| b.join("opencode").exists()).unwrap_or(false)).await, requirements: vec!["Node.js"] },
        CatalogItem { id: "pi", name: "Pi", description: "Pi coding agent", installed: installed(configured("pi").await, node.as_ref().map(|b| b.join("pi").exists()).unwrap_or(false)).await, requirements: vec!["Node.js"] },
        CatalogItem { id: "openclaw", name: "OpenClaw", description: "General purpose agent", installed: configured("openclaw").await.map(|p| FsPath::new(&p).exists()).unwrap_or(false), requirements: vec![] },
    ])
}

#[derive(Serialize)]
pub struct RuntimeSettings {
    pub node_path: Option<String>,
    pub agent_paths: HashMap<String, String>,
}

pub async fn get_runtime_settings(State(s): State<AppState>) -> Json<RuntimeSettings> {
    let agent_paths = setting(&s.db, "agent_paths").await
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default();
    Json(RuntimeSettings { node_path: setting(&s.db, "node_path").await, agent_paths })
}

#[derive(Deserialize)]
pub struct UpdateRuntimeSettings {
    pub node_path: Option<String>,
    pub agent_paths: HashMap<String, String>,
}

pub async fn update_runtime_settings(
    State(s): State<AppState>,
    Json(v): Json<UpdateRuntimeSettings>,
) -> Result<Json<RuntimeSettings>, StatusCode> {
    let node_path = v.node_path.and_then(|p| {
        let p = p.trim().to_owned();
        (!p.is_empty()).then_some(p)
    });
    let agent_paths: HashMap<String, String> = v.agent_paths.into_iter()
        .filter_map(|(k, p)| { let p = p.trim().to_owned(); (!k.trim().is_empty() && !p.is_empty()).then_some((k, p)) })
        .collect();
    save_setting(&s.db, "node_path", node_path.as_deref().unwrap_or(""))
        .await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    save_setting(&s.db, "agent_paths", &serde_json::to_string(&agent_paths).map_err(|_| StatusCode::BAD_REQUEST)?)
        .await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(RuntimeSettings { node_path, agent_paths }))
}

#[derive(Serialize)]
pub struct NodeVersions {
    pub available: Vec<&'static str>,
    pub installed: Vec<String>,
    pub active: Option<String>,
    pub configured_path: Option<String>,
}
pub async fn node_versions(State(s): State<AppState>) -> Json<NodeVersions> {
    let installed = runtime::installed_versions().await.unwrap_or_default();
    let configured_path = setting(&s.db, "node_path").await;
    let active = if let Some(path) = configured_node_path(&s.db).await {
        let out = if path.is_file() { Command::new(&path).arg("--version").output().await.ok() } else { Command::new(path.join("node")).arg("--version").output().await.ok() };
        out.and_then(|o| o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().trim_start_matches('v').to_owned()))
    } else {
        installed.first().cloned()
    };
    Json(NodeVersions { available: runtime::supported_node_versions(), installed, active, configured_path })
}
#[derive(Deserialize)]
pub struct InstallNodeRequest { pub version: String }
pub async fn install_node(Json(v): Json<InstallNodeRequest>) -> Result<Json<serde_json::Value>, StatusCode> {
    runtime::install_node(&v.version, |_| {}).await.map_err(|_| StatusCode::BAD_REQUEST)?;
    Ok(Json(serde_json::json!({"status":"installed","version":v.version})))
}
#[derive(Deserialize)]
pub struct CreateAgent { pub name: String, pub kind: String, pub command: String, pub working_directory: Option<String> }
pub async fn list_agents(State(s): State<AppState>) -> Json<Vec<Agent>> {
    let rows = sqlx::query("SELECT id,name,kind,command,working_directory,installed,version FROM agents ORDER BY name").fetch_all(&s.db).await.unwrap_or_default();
    Json(rows.into_iter().map(|r| Agent { id: r.get(0), name: r.get(1), kind: r.get(2), command: r.get(3), working_directory: r.get(4), installed: r.get::<i64,_>(5) != 0, version: r.get(6) }).collect())
}
pub async fn create_agent(State(s): State<AppState>, Json(v): Json<CreateAgent>) -> Json<Agent> {
    let id = Uuid::new_v4().to_string(); let now = Utc::now().to_rfc3339();
    let _=sqlx::query("INSERT INTO agents(id,name,kind,command,working_directory,installed,created_at,updated_at) VALUES(?,?,?,?,?,0,?,?)").bind(&id).bind(&v.name).bind(&v.kind).bind(&v.command).bind(&v.working_directory).bind(&now).bind(&now).execute(&s.db).await;
    Json(Agent { id, name: v.name, kind: v.kind, command: v.command, working_directory: v.working_directory, installed: false, version: None })
}
pub async fn agent_status(Path(id): Path<String>, State(s): State<AppState>) -> Json<serde_json::Value> {
    if id == "codex" {
        let path = configured_agent_path(&s.db, &id).await;
        let out = if let Some(path) = path { Command::new(path).arg("--version").output().await } else { Command::new("codex").arg("--version").output().await };
        return Json(serde_json::json!({"id":id,"installed":out.as_ref().map(|o|o.status.success()).unwrap_or(false),"version":out.ok().map(|o|String::from_utf8_lossy(&o.stdout).trim().to_string())}));
    }
    if ["claude-code", "opencode", "pi"].contains(&id.as_str()) {
        let binary = match id.as_str() { "claude-code" => "claude", "opencode" => "opencode", "pi" => "pi", _ => "" };
        let path = configured_agent_path(&s.db, &id).await;
        let out = if let Some(path) = path { Command::new(path).arg("--version").output().await } else if let Some(bin) = node_bin_dir(&s.db).await { Command::new(bin.join(binary)).arg("--version").output().await } else { return Json(serde_json::json!({"id":id,"installed":false,"version":null,"node_version":null})); };
        return Json(serde_json::json!({"id":id,"installed":out.as_ref().map(|o|o.status.success()).unwrap_or(false),"version":out.ok().map(|o|String::from_utf8_lossy(&o.stdout).trim().to_string())}));
    }
    let row = sqlx::query("SELECT installed,version FROM agents WHERE id=?").bind(&id).fetch_optional(&s.db).await.ok().flatten();
    Json(serde_json::json!({"id":id,"installed":row.as_ref().map(|r|r.get::<i64,_>(0)!=0).unwrap_or(false),"version":row.and_then(|r|r.get::<Option<String>,_>(1))}))
}
pub async fn install_agent(Path(id): Path<String>, State(s): State<AppState>) -> Json<serde_json::Value> {
    let events = s.events.clone(); let db = s.db.clone(); let id2 = id.clone();
    tokio::spawn(async move {
        let Some(node_bin) = node_bin_dir(&db).await else { events.publish(AgentEvent::InstallOutput { agent_id:id2.clone(), text:"Please configure a Node.js path or install a Node.js version from the Web UI first.".into() }); events.publish(AgentEvent::InstallCompleted { agent_id:id2, success:false }); return; };
        let package = match id2.as_str() { "claude-code"=>"@anthropic-ai/claude-code", "opencode"=>"opencode-ai@latest", "pi"=>"@mariozechner/pi-coding-agent", _=>{ events.publish(AgentEvent::InstallCompleted {agent_id:id2,success:false}); return; } };
        let mut c = Command::new(node_bin.join("npm"));
        c.args(["install","-g",package]).env("PATH",format!("{}:{}",node_bin.display(),std::env::var("PATH").unwrap_or_default())).stdout(Stdio::piped()).stderr(Stdio::piped());
        if runtime::detect_installed_node().await.ok().flatten().map(|(v,_)| runtime::node_bin(&v)==node_bin).unwrap_or(false) { if let Some(v)=runtime::detect_installed_node().await.ok().flatten().map(|(v,_)|v) { c.env("NPM_CONFIG_PREFIX",runtime::node_home(&v)); } }
        match c.output().await { Ok(o)=>{ let text=format!("{}{}",String::from_utf8_lossy(&o.stdout),String::from_utf8_lossy(&o.stderr)); events.publish(AgentEvent::InstallOutput{agent_id:id2.clone(),text}); if o.status.success(){let now=Utc::now().to_rfc3339();let _=sqlx::query("UPDATE agents SET installed=1,version=?,updated_at=? WHERE kind=? OR id=?").bind("configured").bind(&now).bind(&id2).bind(&id2).execute(&db).await;} events.publish(AgentEvent::InstallCompleted{agent_id:id2,success:o.status.success()}); }, Err(e)=>{events.publish(AgentEvent::InstallOutput{agent_id:id2.clone(),text:e.to_string()});events.publish(AgentEvent::InstallCompleted{agent_id:id2,success:false});} }
    });
    Json(serde_json::json!({"status":"started","agent_id":id}))
}
