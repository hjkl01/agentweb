use crate::{installation::runtime, state::AppState};
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use tokio::process::Command;

#[derive(Serialize)]
pub struct NodeVersions { pub available: Vec<String>, pub installed: Vec<String>, pub detected: Vec<runtime::DetectedNode>, pub active: Option<String>, pub configured_path: Option<String> }

pub async fn node_versions(State(s): State<AppState>) -> Json<NodeVersions> {
    let available = runtime::available_node_versions().await.unwrap_or_default();
    let installed = runtime::installed_versions().await.unwrap_or_default();
    let detected = runtime::detect_nodes().await.unwrap_or_default();
    let configured_path = sqlx::query("SELECT value FROM runtime_settings WHERE key=?").bind("node_path").fetch_optional(&s.db).await.ok().flatten().map(|row| row.get::<String, _>(0)).filter(|p| !p.trim().is_empty());
    let active = configured_path.as_ref().and_then(|path| detected.iter().find(|node| node.path == *path).map(|node| node.version.clone())).or_else(|| installed.first().cloned()).or_else(|| detected.first().map(|node| node.version.clone()));
    Json(NodeVersions { available, installed, detected, active, configured_path })
}

#[derive(Deserialize)] pub struct InstallNodeRequest { pub version: String }
pub async fn install_node(Json(v): Json<InstallNodeRequest>) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    let requested = v.version.trim();
    let available = runtime::available_node_versions().await.map_err(|_| axum::http::StatusCode::BAD_REQUEST)?;
    let version = available.iter().find(|item| *item == requested || item.starts_with(&format!("{requested}."))).cloned().ok_or(axum::http::StatusCode::BAD_REQUEST)?;
    runtime::install_node(&version, |_| {}).await.map_err(|_| axum::http::StatusCode::BAD_REQUEST)?;
    Ok(Json(serde_json::json!({ "status": "installed", "version": version })))
}

#[derive(Serialize)] pub struct CatalogItem { pub id: &'static str, pub name: &'static str, pub description: &'static str, pub installed: bool, pub requirements: Vec<&'static str>, pub install_command: &'static str }
const AGENTS: &[(&str, &str, &str, &str, &[&str])] = &[
    ("codex", "Codex", "OpenAI coding agent", "npm install -g @openai/codex", &["Node.js"]),
    ("claude-code", "Claude Code", "Anthropic coding agent", "npm install -g @anthropic-ai/claude-code", &["Node.js"]),
    ("qwen-code", "Qwen Code", "Alibaba Qwen coding agent", "npm install -g @qwen-code/qwen-code", &["Node.js"]),
    ("gemini-cli", "Gemini CLI", "Google Gemini coding agent", "npm install -g @google/gemini-cli", &["Node.js"]),
    ("opencode", "OpenCode", "Open-source coding agent", "npm install -g opencode-ai", &["Node.js"]),
    ("pi", "Pi", "Pi coding agent", "npm install -g @mariozechner/pi-coding-agent", &["Node.js"]),
    ("openclaw", "OpenClaw", "General purpose agent", "npm install -g openclaw", &["Node.js"]),
];
async fn command_exists(command: &str) -> bool { Command::new("which").arg(command).output().await.map(|o| o.status.success()).unwrap_or(false) }
pub async fn catalog(State(_s): State<AppState>) -> Json<Vec<CatalogItem>> {
    let mut result = Vec::with_capacity(AGENTS.len());
    for (id, name, description, install_command, requirements) in AGENTS {
        let binary = match *id { "qwen-code" => "qwen", "gemini-cli" => "gemini", other => other };
        result.push(CatalogItem { id: *id, name: *name, description: *description, installed: command_exists(binary).await, requirements: requirements.to_vec(), install_command: *install_command });
    }
    Json(result)
}

#[derive(Deserialize)] pub struct CustomAgentInstall { pub command: String }
pub async fn install_custom_agent(Json(v): Json<CustomAgentInstall>) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    let command = v.command.trim().to_owned();
    if command.is_empty() { return Err(axum::http::StatusCode::BAD_REQUEST); }
    let mut process = Command::new("sh");
    process.args(["-lc", &command]);
    if let Ok(Some((version, _))) = runtime::detect_installed_node().await {
        let node_bin = runtime::node_bin(&version);
        let path = format!("{}:{}", node_bin.display(), std::env::var("PATH").unwrap_or_default());
        process.env("PATH", path).env("NPM_CONFIG_PREFIX", runtime::node_home(&version));
    }
    let output = process.output().await.map_err(|_| axum::http::StatusCode::BAD_REQUEST)?;
    if !output.status.success() { return Err(axum::http::StatusCode::BAD_REQUEST); }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().chars().take(1000).collect::<String>();
    Ok(Json(serde_json::json!({ "status": "installed", "command": command, "output": stdout })))
}
