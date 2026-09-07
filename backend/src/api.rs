use crate::{agents::AgentConfig, events::AgentEvent, installation::runtime, state::AppState};
use axum::extract::ws::{Message, WebSocket};
use axum::{
    extract::{Path, State, WebSocketUpgrade},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use futures::SinkExt;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::{
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
fn runtime_agent_installed(version: &str, id: &str) -> bool {
    runtime::node_bin(version).join(id).exists()
}
pub async fn catalog(State(_s): State<AppState>) -> Json<Vec<CatalogItem>> {
    let codex = Command::new("codex")
        .arg("--version")
        .output()
        .await
        .is_ok();
    let node = runtime::detect_installed_node().await.ok().flatten();
    let node_version = node.as_ref().map(|x| x.0.clone());
    Json(vec![
        CatalogItem {
            id: "codex",
            name: "Codex",
            description: "OpenAI coding agent",
            installed: codex,
            requirements: vec![],
        },
        CatalogItem {
            id: "claude-code",
            name: "Claude Code",
            description: "Anthropic coding agent",
            installed: node_version
                .as_ref()
                .map(|v| runtime_agent_installed(v, "claude"))
                .unwrap_or(false),
            requirements: vec!["Node.js"],
        },
        CatalogItem {
            id: "opencode",
            name: "OpenCode",
            description: "Open-source coding agent",
            installed: node_version
                .as_ref()
                .map(|v| runtime_agent_installed(v, "opencode"))
                .unwrap_or(false),
            requirements: vec!["Node.js"],
        },
        CatalogItem {
            id: "pi",
            name: "Pi",
            description: "Pi coding agent",
            installed: node_version
                .as_ref()
                .map(|v| runtime_agent_installed(v, "pi"))
                .unwrap_or(false),
            requirements: vec!["Node.js"],
        },
        CatalogItem {
            id: "openclaw",
            name: "OpenClaw",
            description: "General purpose agent",
            installed: false,
            requirements: vec![],
        },
    ])
}
#[derive(Serialize)]
pub struct NodeVersions {
    pub available: Vec<&'static str>,
    pub installed: Vec<String>,
    pub active: Option<String>,
}
pub async fn node_versions() -> Json<NodeVersions> {
    let installed = runtime::installed_versions().await.unwrap_or_default();
    Json(NodeVersions {
        available: runtime::supported_node_versions(),
        active: installed.first().cloned(),
        installed,
    })
}
#[derive(Deserialize)]
pub struct InstallNodeRequest {
    pub version: String,
}
pub async fn install_node(
    Json(v): Json<InstallNodeRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    runtime::install_node(&v.version, |_| {})
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    Ok(Json(
        serde_json::json!({"status":"installed","version":v.version}),
    ))
}
#[derive(Deserialize)]
pub struct CreateAgent {
    pub name: String,
    pub kind: String,
    pub command: String,
    pub working_directory: Option<String>,
}
pub async fn list_agents(State(s): State<AppState>) -> Json<Vec<Agent>> {
    let rows = sqlx::query(
        "SELECT id,name,kind,command,working_directory,installed,version FROM agents ORDER BY name",
    )
    .fetch_all(&s.db)
    .await
    .unwrap_or_default();
    Json(
        rows.into_iter()
            .map(|r| Agent {
                id: r.get(0),
                name: r.get(1),
                kind: r.get(2),
                command: r.get(3),
                working_directory: r.get(4),
                installed: r.get::<i64, _>(5) != 0,
                version: r.get(6),
            })
            .collect(),
    )
}
pub async fn create_agent(State(s): State<AppState>, Json(v): Json<CreateAgent>) -> Json<Agent> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let _=sqlx::query("INSERT INTO agents(id,name,kind,command,working_directory,installed,created_at,updated_at) VALUES(?,?,?,?,?,0,?,?)").bind(&id).bind(&v.name).bind(&v.kind).bind(&v.command).bind(&v.working_directory).bind(&now).bind(&now).execute(&s.db).await;
    Json(Agent {
        id,
        name: v.name,
        kind: v.kind,
        command: v.command,
        working_directory: v.working_directory,
        installed: false,
        version: None,
    })
}
pub async fn agent_status(
    Path(id): Path<String>,
    State(s): State<AppState>,
) -> Json<serde_json::Value> {
    if id == "codex" {
        let out = Command::new("codex").arg("--version").output().await;
        return Json(
            serde_json::json!({"id":id,"installed":out.as_ref().map(|o|o.status.success()).unwrap_or(false),"version":out.ok().map(|o|String::from_utf8_lossy(&o.stdout).trim().to_string())}),
        );
    }
    if ["claude-code", "opencode", "pi"].contains(&id.as_str()) {
        let node = runtime::detect_installed_node().await.ok().flatten();
        if let Some((version, _)) = node {
            let binary = match id.as_str() {
                "claude-code" => "claude",
                "opencode" => "opencode",
                "pi" => "pi",
                _ => "",
            };
            let out = Command::new(runtime::node_bin(&version).join(binary))
                .arg("--version")
                .output()
                .await;
            return Json(
                serde_json::json!({"id":id,"installed":out.as_ref().map(|o|o.status.success()).unwrap_or(false),"version":out.ok().map(|o|String::from_utf8_lossy(&o.stdout).trim().to_string()),"node_version":version}),
            );
        }
        return Json(
            serde_json::json!({"id":id,"installed":false,"version":null,"node_version":null}),
        );
    }
    let row = sqlx::query("SELECT installed,version FROM agents WHERE id=?")
        .bind(&id)
        .fetch_optional(&s.db)
        .await
        .ok()
        .flatten();
    Json(
        serde_json::json!({"id":id,"installed":row.as_ref().map(|r|r.get::<i64,_>(0)!=0).unwrap_or(false),"version":row.and_then(|r|r.get::<Option<String>,_>(1))}),
    )
}
pub async fn install_agent(
    Path(id): Path<String>,
    State(s): State<AppState>,
) -> Json<serde_json::Value> {
    let events = s.events.clone();
    let db = s.db.clone();
    let id2 = id.clone();
    tokio::spawn(async move {
        let Some((node_version, _)) = runtime::detect_installed_node().await.ok().flatten() else {
            events.publish(AgentEvent::InstallOutput {
                agent_id: id2.clone(),
                text: "Please install a Node.js version from the Web UI first.".into(),
            });
            events.publish(AgentEvent::InstallCompleted {
                agent_id: id2,
                success: false,
            });
            return;
        };
        let package = match id2.as_str() {
            "claude-code" => "@anthropic-ai/claude-code",
            "opencode" => "opencode-ai@latest",
            "pi" => "@mariozechner/pi-coding-agent",
            _ => {
                events.publish(AgentEvent::InstallCompleted {
                    agent_id: id2,
                    success: false,
                });
                return;
            }
        };
        let mut c = Command::new(runtime::node_bin(&node_version).join("npm"));
        c.args(["install", "-g", package])
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    runtime::node_bin(&node_version).display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            )
            .env("NPM_CONFIG_PREFIX", runtime::node_home(&node_version))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        match c.output().await {
            Ok(o) => {
                let text = format!(
                    "{}{}",
                    String::from_utf8_lossy(&o.stdout),
                    String::from_utf8_lossy(&o.stderr)
                );
                events.publish(AgentEvent::InstallOutput {
                    agent_id: id2.clone(),
                    text,
                });
                if o.status.success() {
                    let now = Utc::now().to_rfc3339();
                    let _ = sqlx::query(
                        "UPDATE agents SET installed=1,version=?,updated_at=? WHERE kind=? OR id=?",
                    )
                    .bind(&node_version)
                    .bind(&now)
                    .bind(&id2)
                    .bind(&id2)
                    .execute(&db)
                    .await;
                }
                events.publish(AgentEvent::InstallCompleted {
                    agent_id: id2,
                    success: o.status.success(),
                });
            }
            Err(e) => {
                events.publish(AgentEvent::InstallOutput {
                    agent_id: id2.clone(),
                    text: e.to_string(),
                });
                events.publish(AgentEvent::InstallCompleted {
                    agent_id: id2,
                    success: false,
                });
            }
        }
    });
    Json(serde_json::json!({"status":"started","agent_id":id}))
}
#[derive(Serialize)]
pub struct Session {
    pub id: String,
    pub agent_id: String,
    pub title: String,
    pub workspace: String,
    pub status: String,
    pub native_session_id: Option<String>,
}
#[derive(Deserialize)]
pub struct CreateSession {
    pub agent_id: String,
    pub title: Option<String>,
    pub workspace: String,
}
pub async fn list_sessions(State(s): State<AppState>) -> Json<Vec<Session>> {
    let rows=sqlx::query("SELECT id,agent_id,title,workspace,status,native_session_id FROM sessions ORDER BY updated_at DESC").fetch_all(&s.db).await.unwrap_or_default();
    Json(
        rows.into_iter()
            .map(|r| Session {
                id: r.get(0),
                agent_id: r.get(1),
                title: r.get(2),
                workspace: r.get(3),
                status: r.get(4),
                native_session_id: r.get(5),
            })
            .collect(),
    )
}
pub async fn create_session(
    State(s): State<AppState>,
    Json(v): Json<CreateSession>,
) -> Json<Session> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let title = v.title.unwrap_or_else(|| "New Chat".into());
    let _=sqlx::query("INSERT INTO sessions(id,agent_id,title,workspace,status,native_session_id,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?)").bind(&id).bind(&v.agent_id).bind(&title).bind(&v.workspace).bind("created").bind(None::<String>).bind(&now).bind(&now).execute(&s.db).await;
    Json(Session {
        id,
        agent_id: v.agent_id,
        title,
        workspace: v.workspace,
        status: "created".into(),
        native_session_id: None,
    })
}
pub async fn get_session(
    Path(id): Path<String>,
    State(s): State<AppState>,
) -> Result<Json<Session>, StatusCode> {
    let r = sqlx::query(
        "SELECT id,agent_id,title,workspace,status,native_session_id FROM sessions WHERE id=?",
    )
    .bind(&id)
    .fetch_optional(&s.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(Session {
        id: r.get(0),
        agent_id: r.get(1),
        title: r.get(2),
        workspace: r.get(3),
        status: r.get(4),
        native_session_id: r.get(5),
    }))
}
pub async fn delete_session(Path(id): Path<String>, State(s): State<AppState>) -> StatusCode {
    let _ = sqlx::query("DELETE FROM messages WHERE session_id=?")
        .bind(&id)
        .execute(&s.db)
        .await;
    let _ = sqlx::query("DELETE FROM sessions WHERE id=?")
        .bind(id)
        .execute(&s.db)
        .await;
    StatusCode::NO_CONTENT
}
#[derive(Serialize)]
pub struct MessageItem {
    pub id: String,
    pub role: String,
    pub content: String,
    pub created_at: String,
}
pub async fn list_messages(
    Path(id): Path<String>,
    State(s): State<AppState>,
) -> Json<Vec<MessageItem>> {
    let rows=sqlx::query("SELECT id,role,content,created_at FROM messages WHERE session_id=? ORDER BY created_at ASC").bind(&id).fetch_all(&s.db).await.unwrap_or_default();
    Json(
        rows.into_iter()
            .map(|r| MessageItem {
                id: r.get(0),
                role: r.get(1),
                content: r.get(2),
                created_at: r.get(3),
            })
            .collect(),
    )
}
#[derive(Deserialize)]
pub struct MessageReq {
    pub message: String,
}
pub async fn send_message(
    Path(id): Path<String>,
    State(s): State<AppState>,
    Json(v): Json<MessageReq>,
) -> Json<serde_json::Value> {
    let row =
        sqlx::query("SELECT agent_id,workspace,native_session_id,status FROM sessions WHERE id=?")
            .bind(&id)
            .fetch_optional(&s.db)
            .await
            .ok()
            .flatten();
    let Some(r) = row else {
        return Json(serde_json::json!({"error":"session not found"}));
    };
    let agent_id: String = r.get(0);
    let workspace: String = r.get(1);
    let native_session_id: Option<String> = r.get(2);
    let status: String = r.get(3);
    if status == "running" {
        return Json(serde_json::json!({"error":"session is already running"}));
    }
    let _ = sqlx::query("INSERT INTO messages VALUES(?,?,?,?,?)")
        .bind(Uuid::new_v4().to_string())
        .bind(&id)
        .bind("user")
        .bind(&v.message)
        .bind(Utc::now().to_rfc3339())
        .execute(&s.db)
        .await;
    let _ = sqlx::query("UPDATE sessions SET status='running',updated_at=? WHERE id=?")
        .bind(Utc::now().to_rfc3339())
        .bind(&id)
        .execute(&s.db)
        .await;
    s.events.publish(AgentEvent::SessionStarted {
        session_id: id.clone(),
    });
    let command = if agent_id == "codex" {
        "codex".into()
    } else {
        sqlx::query("SELECT command FROM agents WHERE id=?")
            .bind(&agent_id)
            .fetch_optional(&s.db)
            .await
            .ok()
            .flatten()
            .map(|r| r.get(0))
            .unwrap_or_else(|| agent_id.clone())
    };
    let runtime_path = runtime::detect_installed_node()
        .await
        .ok()
        .flatten()
        .map(|(v, _)| runtime::node_bin(&v).to_string_lossy().into_owned());
    let cfg = AgentConfig {
        id: agent_id.clone(),
        command,
        working_directory: Some(workspace),
        native_session_id,
        runtime_path,
    };
    let manager = s.agents.clone();
    let events = s.events.clone();
    let db = s.db.clone();
    let msg = v.message;
    let session_id = id.clone();
    tokio::spawn(async move {
        let adapter = manager.adapter(&cfg.id).await;
        match adapter.send_message(&cfg, &session_id, &msg, &events).await {
            Ok(result) => {
                if let Some(native) = result.native_session_id {
                    let _=sqlx::query("UPDATE sessions SET native_session_id=?,status='completed',updated_at=? WHERE id=?").bind(native).bind(Utc::now().to_rfc3339()).bind(&session_id).execute(&db).await;
                } else {
                    let _ = sqlx::query(
                        "UPDATE sessions SET status='completed',updated_at=? WHERE id=?",
                    )
                    .bind(Utc::now().to_rfc3339())
                    .bind(&session_id)
                    .execute(&db)
                    .await;
                }
                if !result.assistant_text.is_empty() {
                    let _ = sqlx::query("INSERT INTO messages VALUES(?,?,?,?,?)")
                        .bind(Uuid::new_v4().to_string())
                        .bind(&session_id)
                        .bind("assistant")
                        .bind(result.assistant_text)
                        .bind(Utc::now().to_rfc3339())
                        .execute(&db)
                        .await;
                }
            }
            Err(e) => {
                let _ = sqlx::query("UPDATE sessions SET status='error',updated_at=? WHERE id=?")
                    .bind(Utc::now().to_rfc3339())
                    .bind(&session_id)
                    .execute(&db)
                    .await;
                events.publish(AgentEvent::Error {
                    session_id: session_id.clone(),
                    message: e.to_string(),
                });
            }
        }
    });
    Json(serde_json::json!({"status":"started"}))
}
pub async fn interrupt(
    Path(id): Path<String>,
    State(s): State<AppState>,
) -> Json<serde_json::Value> {
    let row = sqlx::query("SELECT agent_id FROM sessions WHERE id=?")
        .bind(&id)
        .fetch_optional(&s.db)
        .await
        .ok()
        .flatten();
    if let Some(r) = row {
        let agent_id: String = r.get(0);
        let _ = s.agents.adapter(&agent_id).await.interrupt(&id).await;
        let _ = sqlx::query("UPDATE sessions SET status='interrupted',updated_at=? WHERE id=?")
            .bind(Utc::now().to_rfc3339())
            .bind(&id)
            .execute(&s.db)
            .await;
    }
    Json(serde_json::json!({"status":"ok"}))
}
#[derive(Serialize)]
pub struct FsEntry {
    pub name: String,
    pub path: String,
    pub kind: String,
    pub size: u64,
}
fn safe_workspace(workspace: &str) -> Result<PathBuf, StatusCode> {
    let root = FsPath::new("/workspaces")
        .canonicalize()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let requested = FsPath::new(workspace);
    let absolute = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        root.join(requested)
    };
    let canonical = absolute.canonicalize().map_err(|_| StatusCode::NOT_FOUND)?;
    if canonical == root || canonical.starts_with(&root) {
        Ok(canonical)
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}
async fn session_workspace(id: &str, s: &AppState) -> Result<PathBuf, StatusCode> {
    let r = sqlx::query("SELECT workspace FROM sessions WHERE id=?")
        .bind(id)
        .fetch_optional(&s.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    safe_workspace(r.get::<String, _>(0).as_str())
}
fn safe_child(root: &FsPath, relative: &str) -> Result<PathBuf, StatusCode> {
    let p = root.join(relative);
    if p.components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(p)
}
pub async fn workspace_files(
    Path(id): Path<String>,
    State(s): State<AppState>,
) -> Result<Json<Vec<FsEntry>>, StatusCode> {
    let root = session_workspace(&id, &s).await?;
    let mut stack = vec![root.clone()];
    let mut out = Vec::new();
    while let Some(dir) = stack.pop() {
        let mut rd = fs::read_dir(&dir)
            .await
            .map_err(|_| StatusCode::NOT_FOUND)?;
        while let Some(e) = rd
            .next_entry()
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        {
            let meta = e
                .metadata()
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let path = e.path();
            let rel = path
                .strip_prefix(&root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', '/');
            if rel.starts_with(".git/") || rel == ".git" {
                continue;
            }
            let kind = if meta.is_dir() { "directory" } else { "file" };
            out.push(FsEntry {
                name: e.file_name().to_string_lossy().into_owned(),
                path: rel.clone(),
                kind: kind.into(),
                size: if meta.is_file() { meta.len() } else { 0 },
            });
            if meta.is_dir() {
                stack.push(path)
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Json(out))
}
pub async fn workspace_file(
    Path((id, path)): Path<(String, String)>,
    State(s): State<AppState>,
) -> Result<axum::response::Response, StatusCode> {
    let root = session_workspace(&id, &s).await?;
    let file = safe_child(&root, &path)?;
    let canonical = file.canonicalize().map_err(|_| StatusCode::NOT_FOUND)?;
    if !canonical.starts_with(&root) || !canonical.is_file() {
        return Err(StatusCode::FORBIDDEN);
    }
    let content = fs::read(&canonical)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if content.len() > 2 * 1024 * 1024 {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    let text = String::from_utf8_lossy(&content).into_owned();
    Ok(Json(serde_json::json!({"path":path,"content":text})).into_response())
}
pub async fn workspace_diff(
    Path(id): Path<String>,
    State(s): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let root = session_workspace(&id, &s).await?;
    let output = Command::new("git")
        .arg("-C")
        .arg(&root)
        .arg("diff")
        .arg("--no-ext-diff")
        .output()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let status = Command::new("git")
        .arg("-C")
        .arg(&root)
        .arg("status")
        .arg("--short")
        .output()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(
        serde_json::json!({"git":output.status.success(),"diff":String::from_utf8_lossy(&output.stdout),"status":String::from_utf8_lossy(&status.stdout)}),
    ))
}
pub async fn ws_events(
    Path(id): Path<String>,
    ws: WebSocketUpgrade,
    State(s): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| websocket(socket, s, id))
}
async fn websocket(mut socket: WebSocket, s: AppState, session_id: String) {
    let mut rx = s.events.subscribe();
    while let Ok(event) = rx.recv().await {
        let matches = match &event {
            AgentEvent::SessionStarted { session_id: s }
            | AgentEvent::MessageStarted { session_id: s }
            | AgentEvent::MessageDelta { session_id: s, .. }
            | AgentEvent::MessageCompleted { session_id: s }
            | AgentEvent::Error { session_id: s, .. }
            | AgentEvent::SessionCompleted { session_id: s } => s == &session_id,
            _ => false,
        };
        if matches {
            if let Ok(text) = serde_json::to_string(&event) {
                if socket.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
        }
    }
}
