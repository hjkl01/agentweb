use crate::{
    agents::{definition, AgentConfig},
    events::AgentEvent,
    installation::runtime,
    state::AppState,
};
use axum::{extract::{Path, State}, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::{collections::HashMap, path::{Path as FsPath, PathBuf}, process::Stdio};
use tokio::process::Command;

pub(crate) async fn setting(db: &sqlx::SqlitePool, key: &str) -> Option<String> { sqlx::query("SELECT value FROM runtime_settings WHERE key=?").bind(key).fetch_optional(db).await.ok().flatten().map(|r| r.get(0)) }
pub(crate) async fn save_setting(db: &sqlx::SqlitePool, key: &str, value: &str) -> Result<(), sqlx::Error> { sqlx::query("INSERT INTO runtime_settings(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value").bind(key).bind(value).execute(db).await.map(|_| ()) }
pub(crate) async fn configured_agent_path(db: &sqlx::SqlitePool, id: &str) -> Option<String> { let raw = setting(db, "agent_paths").await?; let paths: HashMap<String, String> = serde_json::from_str(&raw).ok()?; paths.get(id).cloned().filter(|p| !p.trim().is_empty()) }
pub(crate) async fn configured_node_path(db: &sqlx::SqlitePool) -> Option<PathBuf> { setting(db, "node_path").await.filter(|p| !p.trim().is_empty()).map(PathBuf::from) }
pub(crate) async fn node_bin_dir(db: &sqlx::SqlitePool) -> Option<PathBuf> {
    if let Some(p) = configured_node_path(db).await { if p.is_file() { return p.parent().map(PathBuf::from); } if p.join("node").is_file() { return Some(p); } }
    runtime::detect_installed_node().await.ok().flatten().map(|(v, _)| runtime::node_bin(&v))
}
pub(crate) fn agent_executable(id: &str) -> &str { match id { "codex" => "codex", "claude-code" => "claude", "qwen-code" => "qwen", "gemini-cli" => "gemini", "opencode" => "opencode", "pi" => "pi", "openclaw" => "openclaw", _ => id } }
pub(crate) async fn resolve_agent_binary(db: &sqlx::SqlitePool, id: &str) -> Option<PathBuf> {
    if let Some(p) = configured_agent_path(db, id).await { let path = FsPath::new(&p); if path.is_file() { return Some(path.to_path_buf()); } if path.join(agent_executable(id)).is_file() { return Some(path.join(agent_executable(id))); } }
    if let Some(node_bin) = node_bin_dir(db).await { let binary = node_bin.join(agent_executable(id)); if binary.is_file() { return Some(binary); } }
    let out = Command::new("which").arg(agent_executable(id)).output().await.ok()?;
    out.status.success().then(|| PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()))
}
pub(crate) async fn detect_agent_version(binary: &FsPath) -> Option<String> { let out = Command::new(binary).arg("--version").output().await.ok()?; if !out.status.success() { return None; } let version = String::from_utf8_lossy(&out.stdout).trim().trim_start_matches('v').to_owned(); (!version.is_empty()).then_some(version) }
pub(crate) async fn agent_runtime_label(id: &str, db: &sqlx::SqlitePool) -> Option<String> { if matches!(id, "codex" | "claude-code" | "qwen-code" | "gemini-cli" | "opencode" | "pi") { if let Ok(Some((version, _))) = runtime::detect_installed_node().await { return Some(format!("Node {version}")); } return node_bin_dir(db).await.and_then(|p| p.join("node").exists().then(|| format!("Node ({})", p.display()))); } None }

#[derive(Serialize)] pub struct RuntimeSettings { pub node_path: Option<String>, pub agent_paths: HashMap<String, String>, pub default_agent_id: Option<String>, pub default_model: Option<String> }
pub async fn get_runtime_settings(State(s): State<AppState>) -> Json<RuntimeSettings> {
    let paths = setting(&s.db, "agent_paths").await.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default();
    let default_agent_id = setting(&s.db, "default_agent_id").await.filter(|v| !v.trim().is_empty());
    let default_model = setting(&s.db, "default_model").await.filter(|v| !v.trim().is_empty());
    Json(RuntimeSettings { node_path: setting(&s.db, "node_path").await, agent_paths: paths, default_agent_id, default_model })
}
#[derive(Deserialize)] pub struct UpdateRuntimeSettings { pub node_path: Option<String>, pub agent_paths: HashMap<String, String>, pub default_agent_id: Option<String>, pub default_model: Option<String> }
pub async fn update_runtime_settings(State(s): State<AppState>, Json(v): Json<UpdateRuntimeSettings>) -> Result<Json<RuntimeSettings>, StatusCode> {
    let node_path = v.node_path.map(|p| p.trim().to_owned()).filter(|p| !p.is_empty());
    let paths = v.agent_paths.into_iter().filter_map(|(k, p)| { let p = p.trim().to_owned(); (!k.trim().is_empty() && !p.is_empty()).then_some((k, p)) }).collect::<HashMap<_, _>>();
    let default_agent_id = v.default_agent_id.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
    let default_model = v.default_model.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
    if let Some(agent_id) = &default_agent_id {
        let exists = sqlx::query("SELECT 1 FROM agents WHERE id=?").bind(agent_id).fetch_optional(&s.db).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?.is_some();
        if !exists { return Err(StatusCode::NOT_FOUND); }
    }
    save_setting(&s.db, "node_path", node_path.as_deref().unwrap_or("")).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    save_setting(&s.db, "agent_paths", &serde_json::to_string(&paths).map_err(|_| StatusCode::BAD_REQUEST)?).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    save_setting(&s.db, "default_agent_id", default_agent_id.as_deref().unwrap_or("")).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    save_setting(&s.db, "default_model", default_model.as_deref().unwrap_or("")).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(RuntimeSettings { node_path, agent_paths: paths, default_agent_id, default_model }))
}
pub async fn install_agent(Path(id): Path<String>, State(s): State<AppState>) -> Json<serde_json::Value> { let events = s.events.clone(); let db = s.db.clone(); let id2 = id.clone(); tokio::spawn(async move { let success = install_agent_inner(&db, &id2, &events).await; events.publish(AgentEvent::InstallCompleted { agent_id: id2, success }); }); Json(serde_json::json!({ "status": "started", "agent_id": id })) }
async fn install_agent_inner(db: &sqlx::SqlitePool, id: &str, events: &crate::events::EventBus) -> bool {
    let node_bin = match node_bin_dir(db).await { Some(path) => path, None => { events.publish(AgentEvent::InstallOutput { agent_id: id.to_string(), text: "Please install or configure Node.js first.".into() }); return false; } };
    let package = match id { "codex" => "@openai/codex", "claude-code" => "@anthropic-ai/claude-code", "qwen-code" => "@qwen-code/qwen-code", "gemini-cli" => "@google/gemini-cli", "opencode" => "opencode-ai@latest", "pi" => "@mariozechner/pi-coding-agent", "openclaw" => "openclaw", _ => { events.publish(AgentEvent::InstallOutput { agent_id: id.to_string(), text: format!("Unknown Agent: {id}") }); return false; } };
    let mut cmd = Command::new(node_bin.join("npm")); cmd.args(["install", "-g", package]).env("PATH", format!("{}:{}", node_bin.display(), std::env::var("PATH").unwrap_or_default())).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(node_path) = configured_node_path(db).await { if node_path.is_file() { if let Some(home) = node_path.parent().and_then(|x| x.parent()) { cmd.env("NPM_CONFIG_PREFIX", home); } } }
    events.publish(AgentEvent::InstallOutput { agent_id: id.to_string(), text: format!("Installing {package} …") });
    let output = match cmd.output().await { Ok(output) => output, Err(error) => { events.publish(AgentEvent::InstallOutput { agent_id: id.to_string(), text: format!("npm install failed: {error}") }); return false; } };
    events.publish(AgentEvent::InstallOutput { agent_id: id.to_string(), text: format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)) }); if !output.status.success() { return false; }
    let binary = match resolve_agent_binary(db, id).await { Some(path) => path, None => return false }; let version = match detect_agent_version(&binary).await { Some(v) => v, None => return false };
    let now = chrono::Utc::now().to_rfc3339(); let _ = sqlx::query("UPDATE agents SET installed=1,version=?,updated_at=? WHERE id=?").bind(&version).bind(&now).bind(id).execute(db).await;
    events.publish(AgentEvent::InstallOutput { agent_id: id.to_string(), text: format!("Installed {id} version {version}") }); true
}
pub(crate) async fn build_agent_config(db: &sqlx::SqlitePool, id: &str) -> Result<AgentConfig, StatusCode> {
    let (kind, working_directory) = if let Some(row) = sqlx::query("SELECT kind,working_directory FROM agents WHERE id=?").bind(id).fetch_optional(db).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)? { (row.get::<String, _>(0), row.get::<Option<String>, _>(1)) } else if let Some(def) = definition::BUILT_IN_AGENTS.iter().find(|a| a.id == id) { (def.kind.to_owned(), None) } else { return Err(StatusCode::NOT_FOUND); };
    let binary = resolve_agent_binary(db, id).await.ok_or(StatusCode::NOT_FOUND)?; let runtime_path = node_bin_dir(db).await.map(|p| p.to_string_lossy().into_owned());
    Ok(AgentConfig { id: kind, command: binary.to_string_lossy().into_owned(), working_directory, native_session_id: None, runtime_path, model: None })
}
