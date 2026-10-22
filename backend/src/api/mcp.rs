use super::runtime::build_agent_config;
use crate::{
    agents::{
        mcp::{McpAddRequest, McpView},
        AgentConfig,
    },
    state::AppState,
};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

type McpResult = Result<Json<McpView>, (StatusCode, Json<Value>)>;

fn error(status: StatusCode, message: impl Into<String>) -> (StatusCode, Json<Value>) {
    (
        status,
        Json(json!({"error":{"code":"MCP","message": message.into()}})),
    )
}

#[derive(Deserialize)]
pub struct NameQuery {
    name: String,
}

/// MCP configuration is Pi-specific; other Agents manage their own servers.
async fn pi_config(s: &AppState, id: &str) -> Result<AgentConfig, (StatusCode, Json<Value>)> {
    let config = build_agent_config(&s.db, id)
        .await
        .map_err(|status| error(status, "未知 Agent"))?;
    if config.id != "pi" {
        return Err(error(
            StatusCode::NOT_IMPLEMENTED,
            "只有 Pi Agent 支持 MCP 管理",
        ));
    }
    Ok(config)
}

/// Pi reports rejected configuration on stderr with a non-zero status; pass it through.
fn failed(status: StatusCode, err: anyhow::Error) -> (StatusCode, Json<Value>) {
    error(status, err.to_string())
}

pub async fn list_mcp(Path(id): Path<String>, State(s): State<AppState>) -> McpResult {
    let config = pi_config(&s, &id).await?;
    crate::agents::mcp::list(&config)
        .await
        .map(Json)
        .map_err(|error| failed(StatusCode::BAD_GATEWAY, error))
}

pub async fn add_mcp(
    Path(id): Path<String>,
    State(s): State<AppState>,
    Json(request): Json<McpAddRequest>,
) -> McpResult {
    let args = request
        .arguments()
        .map_err(|message| error(StatusCode::BAD_REQUEST, message))?;
    let config = pi_config(&s, &id).await?;
    crate::agents::mcp::add(&config, args)
        .await
        .map(Json)
        .map_err(|error| failed(StatusCode::BAD_GATEWAY, error))
}

pub async fn remove_mcp(
    Path(id): Path<String>,
    Query(query): Query<NameQuery>,
    State(s): State<AppState>,
) -> McpResult {
    let config = pi_config(&s, &id).await?;
    crate::agents::mcp::remove(&config, &query.name)
        .await
        .map(Json)
        .map_err(|error| failed(StatusCode::BAD_REQUEST, error))
}

pub async fn logout_mcp(
    Path(id): Path<String>,
    Query(query): Query<NameQuery>,
    State(s): State<AppState>,
) -> McpResult {
    let config = pi_config(&s, &id).await?;
    crate::agents::mcp::logout(&config, &query.name)
        .await
        .map(Json)
        .map_err(|error| failed(StatusCode::BAD_REQUEST, error))
}
