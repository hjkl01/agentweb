use crate::{events::AgentEvent, state::AppState};
use axum::{extract::{ws::WebSocket, Path, State, WebSocketUpgrade}, http::StatusCode, response::IntoResponse, Json};
use chrono::Utc;
use futures::SinkExt;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::path::Path as FsPath;
use tokio::fs;
use uuid::Uuid;

#[derive(Serialize, Clone)]
pub struct Session {
    pub id: String,
    pub agent_id: String,
    pub title: String,
    pub workspace: String,
    pub status: String,
    pub native_session_id: Option<String>,
    pub model: Option<String>,
}

async fn load_session(db: &sqlx::SqlitePool, id: &str) -> Result<Session, StatusCode> {
    let row = sqlx::query("SELECT id,agent_id,title,workspace,status,native_session_id,model FROM sessions WHERE id=?")
        .bind(id).fetch_optional(db).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?.ok_or(StatusCode::NOT_FOUND)?;
    Ok(Session { id: row.get(0), agent_id: row.get(1), title: row.get(2), workspace: row.get(3), status: row.get(4), native_session_id: row.get(5), model: row.get(6) })
}

pub async fn list_sessions(State(s): State<AppState>) -> Json<Vec<Session>> {
    let rows = sqlx::query("SELECT id,agent_id,title,workspace,status,native_session_id,model FROM sessions ORDER BY updated_at DESC")
        .fetch_all(&s.db).await.unwrap_or_default();
    Json(rows.into_iter().map(|r| Session { id: r.get(0), agent_id: r.get(1), title: r.get(2), workspace: r.get(3), status: r.get(4), native_session_id: r.get(5), model: r.get(6) }).collect())
}

#[derive(Deserialize)]
pub struct CreateSession { pub agent_id: String, pub title: Option<String>, pub workspace: Option<String>, pub model: Option<String> }

async fn create_workspace(session_id: &str, requested: Option<&str>) -> Result<String, StatusCode> {
    let base_path = std::env::var("AGENTWEB_WORKSPACE_DIR").unwrap_or_else(|_| "./workspaces".into());
    fs::create_dir_all(&base_path).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let base = fs::canonicalize(&base_path).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let name = requested.unwrap_or("").trim();
    let relative = if name.is_empty() || name == "." { session_id.to_owned() } else { name.to_owned() };
    let relative_path = FsPath::new(&relative);
    if relative_path.is_absolute() || relative_path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let workspace = base.join(relative_path);
    fs::create_dir_all(&workspace).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let canonical = fs::canonicalize(&workspace).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !canonical.starts_with(&base) { return Err(StatusCode::FORBIDDEN); }
    Ok(canonical.to_string_lossy().into_owned())
}

pub async fn create_session(State(s): State<AppState>, Json(v): Json<CreateSession>) -> Result<Json<Session>, StatusCode> {
    let agent_exists = sqlx::query("SELECT 1 FROM agents WHERE id=?").bind(&v.agent_id).fetch_optional(&s.db).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?.is_some();
    if !agent_exists { return Err(StatusCode::NOT_FOUND); }
    let id = Uuid::new_v4().to_string();
    let workspace = create_workspace(&id, v.workspace.as_deref()).await?;
    let now = Utc::now().to_rfc3339();
    let title = v.title.filter(|t| !t.trim().is_empty()).unwrap_or_else(|| "New Chat".into());
    sqlx::query("INSERT INTO sessions(id,agent_id,title,workspace,status,native_session_id,model,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?)")
        .bind(&id).bind(&v.agent_id).bind(&title).bind(&workspace).bind("idle")
        .bind::<Option<String>>(None).bind(&v.model).bind(&now).bind(&now).execute(&s.db).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(Session { id, agent_id: v.agent_id, title, workspace, status: "idle".into(), native_session_id: None, model: v.model }))
}

pub async fn get_session(Path(id): Path<String>, State(s): State<AppState>) -> Result<Json<Session>, StatusCode> {
    Ok(Json(load_session(&s.db, &id).await?))
}

pub async fn delete_session(Path(id): Path<String>, State(s): State<AppState>) -> StatusCode {
    let _ = s.agents.interrupt(&id).await;
    match sqlx::query("DELETE FROM sessions WHERE id=?").bind(id).execute(&s.db).await { Ok(_) => StatusCode::NO_CONTENT, Err(_) => StatusCode::INTERNAL_SERVER_ERROR }
}

#[derive(Deserialize)]
pub struct SetModel { pub model: Option<String> }

pub async fn set_session_model(Path(id): Path<String>, State(s): State<AppState>, Json(v): Json<SetModel>) -> Result<Json<Session>, StatusCode> {
    let session = load_session(&s.db, &id).await?;
    if session.status == "running" { return Err(StatusCode::CONFLICT); }
    sqlx::query("UPDATE sessions SET model=?,updated_at=? WHERE id=?").bind(&v.model).bind(Utc::now().to_rfc3339()).bind(&id).execute(&s.db).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(load_session(&s.db, &id).await?))
}

pub async fn list_messages(Path(id): Path<String>, State(s): State<AppState>) -> Result<Json<Vec<serde_json::Value>>, StatusCode> {
    let rows = sqlx::query("SELECT id,role,content,created_at FROM messages WHERE session_id=? ORDER BY created_at")
        .bind(id).fetch_all(&s.db).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(rows.into_iter().map(|r| serde_json::json!({"id":r.get::<String,_>(0),"role":r.get::<String,_>(1),"content":r.get::<String,_>(2),"created_at":r.get::<String,_>(3)})).collect()))
}

#[derive(Deserialize)]
pub struct SendMessage { pub message: String }

pub async fn send_message(Path(id): Path<String>, State(s): State<AppState>, Json(v): Json<SendMessage>) -> Result<Json<serde_json::Value>, StatusCode> {
    if v.message.trim().is_empty() { return Err(StatusCode::BAD_REQUEST); }
    let session = load_session(&s.db, &id).await?;
    if session.status == "running" { return Err(StatusCode::CONFLICT); }
    sqlx::query("INSERT INTO messages(id,session_id,role,content,created_at) VALUES(?,?,?,?,?)")
        .bind(Uuid::new_v4().to_string()).bind(&id).bind("user").bind(&v.message).bind(Utc::now().to_rfc3339()).execute(&s.db).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let events = s.events.clone();
    let db = s.db.clone();
    let agents = s.agents.clone();
    tokio::spawn(async move { crate::agents::process::run_session(db, agents, session, v.message, events).await; });
    Ok(Json(serde_json::json!({"status":"started"})))
}

pub async fn interrupt(Path(id): Path<String>, State(s): State<AppState>) -> Result<Json<serde_json::Value>, StatusCode> {
    let session = load_session(&s.db, &id).await?;
    if session.status != "running" { return Ok(Json(serde_json::json!({"status":session.status}))); }
    s.agents.interrupt(&id).await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok(Json(serde_json::json!({"status":"interrupted"})))
}

fn event_session_id(event: &AgentEvent) -> Option<&str> {
    match event {
        AgentEvent::SessionStarted { session_id } | AgentEvent::MessageStarted { session_id } |
        AgentEvent::MessageDelta { session_id, .. } | AgentEvent::MessageCompleted { session_id } |
        AgentEvent::ThinkingStarted { session_id } | AgentEvent::ThinkingDelta { session_id, .. } |
        AgentEvent::ThinkingCompleted { session_id } | AgentEvent::ToolStarted { session_id, .. } |
        AgentEvent::ToolOutput { session_id, .. } | AgentEvent::ToolCompleted { session_id, .. } |
        AgentEvent::FileCreated { session_id, .. } | AgentEvent::FileModified { session_id, .. } |
        AgentEvent::FileDeleted { session_id, .. } | AgentEvent::CommandStarted { session_id, .. } |
        AgentEvent::CommandOutput { session_id, .. } | AgentEvent::CommandCompleted { session_id } |
        AgentEvent::Error { session_id, .. } | AgentEvent::SessionCompleted { session_id } => Some(session_id),
        AgentEvent::InstallOutput { .. } | AgentEvent::InstallCompleted { .. } => None,
    }
}

pub async fn ws_events(Path(id): Path<String>, ws: WebSocketUpgrade, State(s): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| websocket(socket, s, id))
}

async fn websocket(mut socket: WebSocket, s: AppState, session_id: String) {
    let mut rx = s.events.subscribe();
    while let Ok(event) = rx.recv().await {
        if event_session_id(&event) != Some(session_id.as_str()) { continue; }
        let text = match serde_json::to_string(&event) { Ok(text) => text, Err(_) => continue };
        if socket.send(axum::extract::ws::Message::Text(text.into())).await.is_err() { break; }
    }
}
