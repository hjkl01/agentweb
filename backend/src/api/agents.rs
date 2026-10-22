use super::runtime::{
    agent_runtime_label, build_agent_config, detect_agent_version, resolve_agent_binary,
};
use crate::{
    agents::models::{valid_cli_token, AgentModel, ProviderAuth},
    state::AppState,
};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
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
    let rows = sqlx::query(
        "SELECT id,name,kind,command,working_directory,installed,version FROM agents ORDER BY name",
    )
    .fetch_all(&s.db)
    .await
    .unwrap_or_default();
    let mut agents = Vec::with_capacity(rows.len());
    for r in rows {
        let id: String = r.get(0);
        let db_installed = r.get::<i64, _>(5) != 0;
        let db_version: Option<String> = r.get(6);
        let detected = resolve_agent_binary(&s.db, &id).await;
        let detected_version = match detected.as_deref() {
            Some(path) => detect_agent_version(path).await,
            None => None,
        };
        agents.push(Agent {
            id,
            name: r.get(1),
            kind: r.get(2),
            command: r.get(3),
            working_directory: r.get(4),
            installed: db_installed || detected.is_some(),
            version: detected_version.or(db_version),
        });
    }
    Json(agents)
}

#[derive(Deserialize)]
pub struct CreateAgent {
    pub name: String,
    pub kind: String,
    pub command: String,
    pub working_directory: Option<String>,
}

pub async fn create_agent(
    State(s): State<AppState>,
    Json(v): Json<CreateAgent>,
) -> Result<Json<Agent>, StatusCode> {
    if v.name.trim().is_empty() || v.kind.trim().is_empty() || v.command.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    sqlx::query("INSERT INTO agents(id,name,kind,command,working_directory,installed,created_at,updated_at) VALUES(?,?,?,?,?,0,?,?)")
        .bind(&id).bind(&v.name).bind(&v.kind).bind(&v.command).bind(&v.working_directory).bind(&now).bind(&now)
        .execute(&s.db).await.map_err(|_| StatusCode::CONFLICT)?;
    Ok(Json(Agent {
        id,
        name: v.name,
        kind: v.kind,
        command: v.command,
        working_directory: v.working_directory,
        installed: false,
        version: None,
    }))
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
    let runtime = if binary.is_some() {
        agent_runtime_label(&id, &s.db).await
    } else {
        None
    };
    Json(AgentStatus {
        id,
        installed: binary.is_some(),
        version,
        path: binary.map(|p| p.to_string_lossy().into_owned()),
        runtime,
    })
}

pub async fn agent_models(
    Path(id): Path<String>,
    State(s): State<AppState>,
) -> Result<Json<Vec<AgentModel>>, StatusCode> {
    let cfg = build_agent_config(&s.db, &id).await?;
    let models = s
        .agents
        .adapter(&cfg.id)
        .await
        .list_models(&cfg)
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok(Json(models))
}

#[derive(Deserialize)]
pub struct AuthQuery {
    provider: String,
}

pub async fn agent_auth(
    Path(id): Path<String>,
    Query(query): Query<AuthQuery>,
    State(s): State<AppState>,
) -> Result<Json<ProviderAuth>, StatusCode> {
    let provider = query.provider.trim();
    if !valid_cli_token(provider) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let cfg = build_agent_config(&s.db, &id).await?;
    if cfg.id != "pi" {
        return Err(StatusCode::NOT_IMPLEMENTED);
    }
    let auth = crate::agents::models::check_auth(&cfg, provider)
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok(Json(auth))
}

#[derive(Deserialize)]
pub struct CommandsQuery {
    cwd: Option<String>,
}

/// Skills, prompt templates, and extension commands Pi loads for one working directory.
pub async fn agent_commands(
    Path(id): Path<String>,
    Query(query): Query<CommandsQuery>,
    State(s): State<AppState>,
) -> Result<Json<Vec<crate::agents::pi_rpc::CommandInfo>>, (StatusCode, String)> {
    let mut cfg = build_agent_config(&s.db, &id)
        .await
        .map_err(|status| (status, "未知 Agent".to_owned()))?;
    if cfg.id != "pi" {
        return Err((
            StatusCode::NOT_IMPLEMENTED,
            "只有 Pi Agent 支持列出命令与技能".to_owned(),
        ));
    }
    // Project-scoped resources depend on the cwd, so it has to stay inside the workspace root.
    let cwd = query.cwd.as_deref().map(str::trim).unwrap_or_default();
    if !cwd.is_empty() {
        let path = super::worktrees::allowed_cwd(cwd)
            .await
            .map_err(|status| (status, "工作目录必须在允许的根目录之内".to_owned()))?;
        cfg.working_directory = Some(path.to_string_lossy().into_owned());
    }
    crate::agents::pi_rpc::list_commands(&cfg)
        .await
        .map(Json)
        .map_err(|error| (StatusCode::BAD_GATEWAY, error.to_string()))
}
