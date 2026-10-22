use super::runtime::build_agent_config;
use crate::{
    agents::{plugins::PackageInfo, AgentConfig},
    state::AppState,
};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

type PluginsResult = Result<Json<Vec<PackageInfo>>, (StatusCode, Json<Value>)>;

fn error(status: StatusCode, message: impl Into<String>) -> (StatusCode, Json<Value>) {
    (
        status,
        Json(json!({"error":{"code":"PLUGINS","message": message.into()}})),
    )
}

#[derive(Deserialize)]
pub struct PluginsQuery {
    cwd: Option<String>,
}

#[derive(Deserialize)]
pub struct PluginsAction {
    action: String,
    #[serde(default)]
    source: Option<String>,
    /// Install into `.pi/settings.json` of the working directory instead of the user settings.
    #[serde(default)]
    local: bool,
}

/// Packages are a Pi concept; other Agents manage their own extensions.
async fn pi_config(
    s: &AppState,
    id: &str,
    cwd: Option<&str>,
) -> Result<AgentConfig, (StatusCode, Json<Value>)> {
    let mut config = build_agent_config(&s.db, id)
        .await
        .map_err(|status| error(status, "未知 Agent"))?;
    if config.id != "pi" {
        return Err(error(
            StatusCode::NOT_IMPLEMENTED,
            "只有 Pi Agent 支持插件管理",
        ));
    }
    // Project-scoped packages live in the working directory, so it must stay inside the
    // allowed workspace roots.
    let cwd = cwd.map(str::trim).unwrap_or_default();
    if !cwd.is_empty() {
        let path = super::worktrees::allowed_cwd(cwd)
            .await
            .map_err(|status| error(status, "工作目录必须在允许的根目录之内"))?;
        config.working_directory = Some(path.to_string_lossy().into_owned());
    }
    Ok(config)
}

/// Pi refuses project-scope edits in a directory it does not trust and reports that on stderr;
/// pass its message through instead of inventing a policy of our own.
fn failed(err: anyhow::Error) -> (StatusCode, Json<Value>) {
    error(StatusCode::BAD_GATEWAY, err.to_string())
}

pub async fn list_plugins(
    Path(id): Path<String>,
    Query(query): Query<PluginsQuery>,
    State(s): State<AppState>,
) -> PluginsResult {
    let config = pi_config(&s, &id, query.cwd.as_deref()).await?;
    crate::agents::plugins::list(&config)
        .await
        .map(Json)
        .map_err(failed)
}

pub async fn plugin_action(
    Path(id): Path<String>,
    Query(query): Query<PluginsQuery>,
    State(s): State<AppState>,
    Json(body): Json<PluginsAction>,
) -> PluginsResult {
    let source = body.source.as_deref().map(str::trim).unwrap_or_default();
    if body.action != "update" && source.is_empty() {
        return Err(error(StatusCode::BAD_REQUEST, "缺少包来源"));
    }
    // A source is a value, never a flag; reject it here so the user gets a 400, not a 502.
    if source.starts_with('-') {
        return Err(error(StatusCode::BAD_REQUEST, "包来源不能以 - 开头"));
    }
    let config = pi_config(&s, &id, query.cwd.as_deref()).await?;
    let result = match body.action.as_str() {
        "install" => crate::agents::plugins::install(&config, source, body.local).await,
        "remove" => crate::agents::plugins::remove(&config, source, body.local).await,
        "update" => {
            let source = (!source.is_empty()).then_some(source);
            crate::agents::plugins::update(&config, source).await
        }
        other => {
            return Err(error(
                StatusCode::BAD_REQUEST,
                format!("不支持的操作：{other}"),
            ))
        }
    };
    result.map(Json).map_err(failed)
}
