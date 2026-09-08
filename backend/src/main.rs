mod agents;
mod api;
mod api_node;
mod db;
mod events;
mod installation;
mod openapi;
mod state;

use anyhow::Result;
use axum::{routing::get, Router};
use sqlx::{sqlite::SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use std::{net::SocketAddr, path::Path, str::FromStr};
use tower_http::services::ServeDir;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env()).init();
    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://./data/agentweb.db".into());
    if let Some(path) = db_url.strip_prefix("sqlite://") {
        let path = path.split('?').next().unwrap_or(path);
        if let Some(parent) = Path::new(path).parent() {
            if !parent.as_os_str().is_empty() { tokio::fs::create_dir_all(parent).await?; }
        }
    }
    let db_options = SqliteConnectOptions::from_str(&db_url)?.create_if_missing(true);
    let pool: SqlitePool = SqlitePoolOptions::new().max_connections(5).connect_with(db_options).await?;
    db::init(&pool).await?;
    if let Some(password) = db::ensure_default_admin(&pool).await? {
        tracing::warn!(username="admin", password=%password, "Created default admin account. Save this password; it will only be shown once.");
    }
    let state = state::AppState::new(pool);
    let frontend_dir = std::env::var("AGENTWEB_FRONTEND_DIR").unwrap_or_else(|_| "./frontend/dist".into());
    let app = Router::new()
        .route("/api/health", get(api::health))
        .route("/api/agents", get(api::list_agents).post(api::create_agent))
        .route("/api/agent-catalog", get(api::catalog))
        .route("/api/runtime/settings", get(api::get_runtime_settings).put(api::update_runtime_settings))
        .route("/api/node/versions", get(api_node::node_versions))
        .route("/api/node/install", axum::routing::post(api::install_node))
        .route("/api/agents/{id}/status", get(api::agent_status))
        .route("/api/agents/{id}/models", get(api::agent_models))
        .route("/api/agents/{id}/install", axum::routing::post(api::install_agent))
        .route("/api/sessions", get(api::list_sessions).post(api::create_session))
        .route("/api/sessions/{id}", get(api::get_session).delete(api::delete_session))
        .route("/api/sessions/{id}/model", axum::routing::put(api::set_session_model))
        .route("/api/sessions/{id}/messages", get(api::list_messages).post(api::send_message))
        .route("/api/sessions/{id}/interrupt", axum::routing::post(api::interrupt))
        .route("/api/sessions/{id}/files", get(api::workspace_files))
        .route("/api/sessions/{id}/file/{*path}", get(api::workspace_file))
        .route("/api/sessions/{id}/diff", get(api::workspace_diff))
        .route("/api/sessions/{id}/events", get(api::ws_events))
        .merge(SwaggerUi::new("/docs").url("/api-doc/openapi.json", openapi::ApiDoc::openapi()))
        .fallback_service(ServeDir::new(frontend_dir))
        .with_state(state);
    let addr: SocketAddr = "0.0.0.0:8080".parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
