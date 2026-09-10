mod agents;
mod api;
mod api_error;
mod api_node;
mod auth;
mod db;
mod events;
mod installation;
mod openapi;
mod state;
mod terminal;

use anyhow::Result;
use axum::{middleware, routing::get, Router};
use chrono::Utc;
use sqlx::{sqlite::{SqliteConnectOptions, SqlitePoolOptions}, SqlitePool};
use std::{path::Path, str::FromStr};
use tower_http::{services::{ServeDir, ServeFile}, trace::TraceLayer};
use tracing_subscriber::EnvFilter;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

#[tokio::main]
async fn main()->Result<()>{
 let filter=EnvFilter::try_from_default_env().unwrap_or_else(|_|EnvFilter::new("info"));
 tracing_subscriber::fmt().with_env_filter(filter).init();
 let db_url=std::env::var("DATABASE_URL").unwrap_or_else(|_|"sqlite://./data/agentweb.db".into());
 if let Some(path)=db_url.strip_prefix("sqlite://"){let path=path.split('?').next().unwrap_or(path);if let Some(parent)=Path::new(path).parent(){if !parent.as_os_str().is_empty(){tokio::fs::create_dir_all(parent).await?;}}}
 let db_options=SqliteConnectOptions::from_str(&db_url)?.create_if_missing(true).journal_mode(sqlx::sqlite::SqliteJournalMode::Wal).busy_timeout(std::time::Duration::from_secs(5)); let pool:SqlitePool=SqlitePoolOptions::new().max_connections(5).connect_with(db_options).await?;
 db::init(&pool).await?;
 let now=Utc::now().to_rfc3339(); let _=sqlx::query("UPDATE sessions SET status='interrupted',worker_id=NULL,lease_until=NULL,updated_at=? WHERE status='running' AND (lease_until IS NULL OR lease_until<?)").bind(&now).bind(&now).execute(&pool).await;
 if let Some(password)=db::ensure_default_admin(&pool).await?{eprintln!("\n============================================================");eprintln!(" Agent Web首次启动，已创建默认管理员账号");eprintln!(" 用户名: admin");eprintln!(" 密码:   {password}");eprintln!(" 请立即保存密码；删除数据库后会重新生成新的密码。");eprintln!("============================================================\n");}
 let cleanup_db=pool.clone(); tokio::spawn(async move { let mut ticker=tokio::time::interval(std::time::Duration::from_secs(3600)); loop { ticker.tick().await; let _=sqlx::query("DELETE FROM agent_events WHERE created_at < datetime('now','-1 day') AND (session_id IS NULL OR NOT EXISTS (SELECT 1 FROM sessions WHERE sessions.id=agent_events.session_id AND sessions.status='running'))").execute(&cleanup_db).await; } });
 let state=state::AppState::new(pool); let frontend_dir=std::env::var("AGENTWEB_FRONTEND_DIR").unwrap_or_else(|_|"./frontend/dist".into()); let index_file=Path::new(&frontend_dir).join("index.html");
 let protected_api=Router::new()
  .route("/auth/password",axum::routing::put(auth::change_password)).route("/auth/sessions",get(auth::list_sessions)).route("/auth/sessions/revoke-all",axum::routing::post(auth::revoke_all_sessions))
  .route("/agents",get(api::list_agents).post(api::create_agent)).route("/agent-catalog",get(api_node::catalog)).route("/runtime/settings",get(api::get_runtime_settings).put(api::update_runtime_settings))
  .route("/node/versions",get(api_node::node_versions)).route("/node/install",axum::routing::post(api_node::install_node)).route("/agents/custom/install",axum::routing::post(api_node::install_custom_agent)).route("/agents/{id}/status",get(api::agent_status)).route("/agents/{id}/models",get(api::agent_models)).route("/agents/{id}/install",axum::routing::post(api::install_agent)).route("/agents/{id}/config",get(api::get_agent_config).put(api::save_agent_config))
  .route("/terminal/ws",get(terminal::ws_terminal)).route("/sessions",get(api::list_sessions).post(api::create_session)).route("/sessions/{id}",get(api::get_session).delete(api::delete_session)).route("/sessions/{id}/title",axum::routing::put(api::rename_session)).route("/sessions/{id}/pin",axum::routing::put(api::toggle_pin)).route("/sessions/{id}/model",axum::routing::put(api::set_session_model)).route("/sessions/{id}/agent",axum::routing::put(api::set_session_agent))
  .route("/sessions/{id}/filesystem/tree",get(api::filesystem_tree)).route("/sessions/{id}/messages",get(api::list_messages).post(api::send_message)).route("/sessions/{id}/interrupt",axum::routing::post(api::interrupt)).route("/sessions/{id}/files",get(api::workspace_files)).route("/sessions/{id}/file/{*path}",get(api::workspace_file)).route("/sessions/{id}/diff",get(api::workspace_diff)).route("/sessions/{id}/events",get(api::ws_events))
  .route("/events",get(api::ws_install_events))
  .layer(middleware::from_fn_with_state(state.clone(),auth::basic_auth));
 let api_routes=Router::new().route("/health",get(api::health)).route("/auth/login",axum::routing::post(auth::login)).route("/auth/me",get(auth::me)).route("/auth/logout",axum::routing::post(auth::logout)).merge(protected_api).layer(middleware::from_fn(api_error::normalize));
 let app=Router::new().nest("/api",api_routes).merge(SwaggerUi::new("/docs").url("/api-doc/openapi.json",openapi::ApiDoc::openapi())).fallback_service(ServeDir::new(&frontend_dir).not_found_service(ServeFile::new(index_file))).layer(TraceLayer::new_for_http()).with_state(state);
 let bind_addr=std::env::var("AGENTWEB_BIND_ADDR").unwrap_or_else(|_|"0.0.0.0:8080".into());
 let listener=tokio::net::TcpListener::bind(&bind_addr).await?; tracing::info!("Agent Web listening on {bind_addr}"); axum::serve(listener,app).await?; Ok(())
}