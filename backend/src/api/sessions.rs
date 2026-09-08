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