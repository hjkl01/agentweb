use crate::{agents::config::{self, AgentCapabilities, AgentConfigFile}, state::AppState};
use axum::{extract::{Path, Query, State}, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use sqlx::Row;

#[derive(Debug, Serialize)]
pub struct AgentConfigView {
    pub agent_id: String,
    pub kind: String,
    pub capabilities: AgentCapabilities,
    pub known_paths: Vec<String>,
    pub file: AgentConfigFile,
}

#[derive(Debug, Deserialize)]
pub struct ConfigQuery { pub path: Option<String> }

async fn agent_kind(state: &AppState, id: &str) -> Result<String, StatusCode> {
    if let Some(row) = sqlx::query("SELECT kind FROM agents WHERE id=?").bind(id).fetch_optional(&state.db).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)? {
        return Ok(row.get(0));
    }
    crate::agents::definition::BUILT_IN_AGENTS.iter().find(|agent| agent.id == id).map(|agent| agent.kind.to_owned()).ok_or(StatusCode::NOT_FOUND)
}

pub async fn get_agent_config(Path(id): Path<String>, Query(query): Query<ConfigQuery>, State(state): State<AppState>) -> Result<Json<AgentConfigView>, StatusCode> {
    let kind = agent_kind(&state, &id).await?;
    let file = config::load(&kind, query.path.as_deref()).map_err(|_| StatusCode::BAD_REQUEST)?;
    let known_paths = config::known_paths(&kind).into_iter().map(|p| p.to_string_lossy().into_owned()).collect();
    Ok(Json(AgentConfigView { agent_id: id, kind: kind.clone(), capabilities: config::capabilities(&kind), known_paths, file }))
}

#[derive(Debug, Deserialize)]
pub struct SaveAgentConfig { pub path: String, pub content: String }

pub async fn save_agent_config(Path(id): Path<String>, State(state): State<AppState>, Json(input): Json<SaveAgentConfig>) -> Result<Json<AgentConfigView>, StatusCode> {
    let kind = agent_kind(&state, &id).await?;
    let file = config::save(&kind, &input.path, &input.content).map_err(|error| {
        tracing::warn!(agent=%id, error=%error, "failed to save agent configuration");
        StatusCode::BAD_REQUEST
    })?;
    let known_paths = config::known_paths(&kind).into_iter().map(|p| p.to_string_lossy().into_owned()).collect();
    Ok(Json(AgentConfigView { agent_id: id, kind: kind.clone(), capabilities: config::capabilities(&kind), known_paths, file }))
}
