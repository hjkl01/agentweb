mod agents;
mod api;
mod db;
mod events;
mod installation;
mod state;

use anyhow::Result;
use axum::{routing::get, Router};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use state::AppState;
use std::net::SocketAddr;
use std::str::FromStr;
use tower_http::services::ServeDir;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let workspace_dir = std::env::var("AGENTWEB_WORKSPACE_DIR")
        .unwrap_or_else(|_| "./workspaces".into());
    tokio::fs::create_dir_all(&workspace_dir).await?;
    tokio::fs::create_dir_all(installation::runtime::runtime_root()).await?;

    let db_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://./agentweb.db".into());
    let db_options = SqliteConnectOptions::from_str(&db_url)?.create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(db_options)
        .await?;
    db::init(&pool).await?;

    if let Some(password) = db::ensure_default_admin(&pool).await? {
        tracing::warn!(
            username = "admin",
            password = %password,
            "Created default admin account. Save this password; it will only be shown once."
        );
    }

    let state = AppState::new(pool);
    let app = Router::new()
        .route("/api/health", get(api::health))
        .route("/api/agents", get(api::list_agents).post(api::create_agent))
        .route("/api/agent-catalog", get(api::catalog))
        .route("/api/node/versions", get(api::node_versions))
        .route("/api/node/install", axum::routing::post(api::install_node))
        .route("/api/agents/{id}/status", get(api::agent_status))
        .route(
            "/api/agents/{id}/install",
            axum::routing::post(api::install_agent),
        )
        .route(
            "/api/sessions",
            get(api::list_sessions).post(api::create_session),
        )
        .route(
            "/api/sessions/{id}",
            get(api::get_session).delete(api::delete_session),
        )
        .route(
            "/api/sessions/{id}/messages",
            get(api::list_messages).post(api::send_message),
        )
        .route(
            "/api/sessions/{id}/interrupt",
            axum::routing::post(api::interrupt),
        )
        .route("/api/sessions/{id}/files", get(api::workspace_files))
        .route("/api/sessions/{id}/file/{*path}", get(api::workspace_file))
        .route("/api/sessions/{id}/diff", get(api::workspace_diff))
        .route("/api/sessions/{id}/events", get(api::ws_events))
        .fallback_service(ServeDir::new("/app/frontend"))
        .with_state(state);

    let addr: SocketAddr = "0.0.0.0:8080".parse()?;
    tracing::info!(%addr, "agentweb listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
