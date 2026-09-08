use super::runtime::{build_agent_config, detect_agent_version, resolve_agent_binary};
use crate::{agents::models::AgentModel, state::AppState};
use axum::{extract::{Path, State}, http::StatusCode, Json};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;

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

pub async fn list_agents(State(s): State<AppState>) -> Json<Vec<Agent>> {
    let rows = sqlx::query("SELECT id,name,kind,command,working_directory,installed,version FROM agents ORDER BY name")
        .fetch_all(&s.db).await.unwrap_or_default();
    Json(rows.into_iter().map(|r| Agent {
        id: r.get(0), name: r.get(1), kind: r.get(2), command: r.get(3),
        working_directory: r.get(4), installed: r.get::<i64, _>(5) != 0, version: r.get(6),
    }).collect())
}

#[derive(Deserialize)]
pub struct CreateAgent {
    pub name: String,
    pub kind: String,
    pub command: String,
    pub working_directory: Option<String>,
}

pub async fn create_agent(State(s): State<AppState>, Json(v): Json<CreateAgent>) -> Result<Json<Agent>, StatusCode> {
    if v.name.trim().is_empty() || v.kind.trim().is_empty() || v.command.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    sqlx::query("INSERT INTO agents(id,name,kind,command,working_directory,installed,created_at,updated_at) VALUES(?,?,?,?,?,0,?,?)")
        .bind(&id).bind(&v.name).bind(&v.kind).bind(&v.command).bind(&v.working_directory)
        .bind(&now).bind(&now).execute(&s.db).await.map_err(|_| StatusCode::CONFLICT)?;
    Ok(Json(Agent { id, name: v.name, kind: v.kind, command: v.command, working_directory: v.working_directory, installed: false, version: None }))
}

#[derive(Serialize)]
pub struct AgentStatus {
    pub id: String,
    pub installed: bool,
    pub version: Option<String>,
    pub path: Option<String>,
    pub runtime: Option<String>,
}

pub async fn agent_status(Path(id): Path<String>, State(s): State<AppState>) -> Json<AgentStatus> {
    let binary = resolve_agent_binary(&s.db, &id).await;
    let version = match binary.as_deref() {
        Some(path) => detect_agent_version(path).await,
        None => None,
    };
    Json(AgentStatus {
        id,
        installed: version.is_some(),
        version,
        path: binary.map(|p| p.to_string_lossy().into_owned()),
        runtime: None,
    })
}

pub async fn agent_models(Path(id): Path<String>, State(s): State<AppState>) -> Result<Json<Vec<AgentModel>>, StatusCode> {
    let cfg = build_agent_config(&s.db, &id).await?;
    let models = s.agents.adapter(&cfg.id).await.list_models(&cfg).await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok(Json(models))
}
