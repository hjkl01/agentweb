mod agents;
mod api;
mod db;
mod events;
mod state;

use anyhow::Result;
use axum::{routing::get, Router};
use sqlx::sqlite::SqlitePoolOptions;
use std::net::SocketAddr;
use state::AppState;
use tower_http::services::ServeDir;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter(EnvFilter::from_default_env()).init();
    tokio::fs::create_dir_all("/data").await?;
    tokio::fs::create_dir_all("/workspaces").await?;
    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:///data/agentweb.db".into());
    let pool = SqlitePoolOptions::new().max_connections(5).connect(&db_url).await?;
    db::init(&pool).await?;
    let state = AppState::new(pool);
    let app = Router::new()
        .route("/api/health", get(api::health))
        .route("/api/agents", get(api::list_agents).post(api::create_agent))
        .route("/api/agent-catalog", get(api::catalog))
        .route("/api/agents/{id}/status", get(api::agent_status))
        .route("/api/agents/{id}/install", axum::routing::post(api::install_agent))
        .route("/api/sessions", get(api::list_sessions).post(api::create_session))
        .route("/api/sessions/{id}", get(api::get_session).delete(api::delete_session))
        .route("/api/sessions/{id}/messages", get(api::list_messages).post(api::send_message))
        .route("/api/sessions/{id}/interrupt", axum::routing::post(api::interrupt))
        .route("/api/sessions/{id}/events", get(api::ws_events))
        .fallback_service(ServeDir::new("/app/frontend"))
        .with_state(state);
    let addr: SocketAddr = "0.0.0.0:8080".parse()?;
    tracing::info!(%addr, "agentweb listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
