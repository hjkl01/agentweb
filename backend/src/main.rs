mod agents;
mod api;
mod api_node;
mod auth;
mod db;
mod events;
mod installation;
mod openapi;
mod state;

use anyhow::Result;
use axum::{middleware, routing::get, Router};
use sqlx::{sqlite::{SqliteConnectOptions, SqlitePoolOptions}, SqlitePool};
use std::{net::SocketAddr, path::Path, str::FromStr};
use tower_http::services::{ServeDir, ServeFile};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env()).init();
    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://./data/agentweb.db".into());
    if let Some(path) = db_url.strip_prefix("sqlite://") { let path = path.split('?').next().unwrap_or(path); if let Some(parent) = Path::new(path).parent() { if !parent.as_os_str().is_empty() { tokio::fs::create_dir_all(parent).await?; } } }
    let db_options = SqliteConnectOptions::from_str(&db_url)?.create_if_missing(true);
    let pool: SqlitePool = SqlitePoolOptions::new().max_connections(5).connect_with(db_options).await?;
    db::init(&pool).await?;
    if let Some(password) = db::ensure_default_admin(&pool).await? {
        tracing::warn!(username="admin", password=%password, "Created default admin account. Save this password; it will only be shown once.");
        println!();
        println!("============================================================");
        println!(" Agent Web 首次启动，已创建默认管理员账号");
        println!(" 用户名: admin");
        println!(" 密码:   {password}");
        println!(" 请立即保存密码；删除数据库后会重新生成新的密码。");
        println!("============================================================");
        println!();
    }
    let state = state::AppState::new(pool);
    let frontend_dir = std::env::var("AGENTWEB_FRONTEND_DIR").unwrap_or_else(|_| "./frontend/dist".into());
    let index_file = Path::new(&frontend_dir).join("index.html");
    let protected_api = Router::new()
        .route("/agents", get(api::list_agents).post(api::create_agent))
        .route("/agent-catalog", get(api::catalog))
        .route("/runtime/settings", get(api::get_runtime_settings).put(api::update_runtime_settings))
        .route("/node/versions", get(api_node::node_versions))
        .route("/node/install", axum::routing::post(api::install_node))
        .route("/agents/{id}/status", get(api::agent_status))
        .route("/agents/{id}/models", get(api::agent_models))
        .route("/agents/{id}/install", axum::routing::post(api::install_agent))
        .route("/sessions", get(api::list_sessions).post(api::create_session))
        .route("/sessions/{id}", get(api::get_session).delete(api::delete_session))
        .route("/sessions/{id}/model", axum::routing::put(api::set_session_model))
        .route("/sessions/{id}/messages", get(api::list_messages).post(api::send_message))
        .route("/sessions/{id}/interrupt", axum::routing::post(api::interrupt))
        .route("/sessions/{id}/files", get(api::workspace_files))
        .route("/sessions/{id}/file/{*path}", get(api::workspace_file))
        .route("/sessions/{id}/diff", get(api::workspace_diff))
        .route("/sessions/{id}/events", get(api::ws_events))
        .layer(middleware::from_fn_with_state(state.clone(), auth::basic_auth));
    let app = Router::new()
        .route("/api/health", get(api::health))
        .route("/api/auth/login", axum::routing::post(auth::login))
        .route("/api/auth/me", get(auth::me))
        .nest("/api", protected_api)
        .merge(SwaggerUi::new("/docs").url("/api-doc/openapi.json", openapi::ApiDoc::openapi()))
        .fallback_service(ServeDir::new(&frontend_dir).not_found_service(ServeFile::new(index_file)))
        .with_state(state);
    let addr: SocketAddr = "0.0.0.0:8080".parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}