use super::{adapter::AgentConfig, definition, AgentManager};
use crate::{api::Session, events::{AgentEvent, EventBus}, installation::runtime};
use chrono::Utc;
use sqlx::Row;
use std::sync::Arc;

async fn runtime_path(db: &sqlx::SqlitePool) -> Option<String> {
    if let Ok(Some((version, _))) = runtime::detect_installed_node().await {
        return Some(runtime::node_bin(&version).to_string_lossy().into_owned());
    }
    let configured = sqlx::query("SELECT value FROM runtime_settings WHERE key='node_path'")
        .fetch_optional(db).await.ok().flatten().map(|row| row.get::<String, _>(0));
    let path = configured?.trim().to_owned();
    if path.is_empty() { return None; }
    let path = std::path::PathBuf::from(path);
    if path.is_file() { path.parent().map(|p| p.to_string_lossy().into_owned()) }
    else if path.join("node").is_file() { Some(path.to_string_lossy().into_owned()) }
    else { None }
}

pub async fn run_session(
    db: sqlx::SqlitePool,
    agents: Arc<AgentManager>,
    session: Session,
    message: String,
    events: EventBus,
) {
    let row = sqlx::query("SELECT kind,command FROM agents WHERE id=?")
        .bind(&session.agent_id).fetch_optional(&db).await.ok().flatten();
    let (kind, command) = if let Some(row) = row {
        (row.get(0), row.get(1))
    } else if let Some(def) = definition::BUILT_IN_AGENTS.iter().find(|a| a.id == session.agent_id) {
        (def.kind.to_owned(), def.command.to_owned())
    } else {
        events.publish(AgentEvent::Error { session_id: session.id, message: "agent not found".into() });
        return;
    };

    // The Web Session workspace is authoritative. Agent-level working_directory
    // must never silently move a chat outside the workspace shown in the UI.
    let config = AgentConfig {
        id: kind.clone(),
        command,
        working_directory: Some(session.workspace.clone()),
        native_session_id: session.native_session_id.clone(),
        runtime_path: runtime_path(&db).await,
        model: session.model.clone(),
    };
    let _ = sqlx::query("UPDATE sessions SET status='running',updated_at=? WHERE id=?")
        .bind(Utc::now().to_rfc3339()).bind(&session.id).execute(&db).await;
    let adapter = agents.adapter(&kind).await;
    match adapter.send_message(&config, &session.id, &message, &events).await {
        Ok(result) => {
            let now = Utc::now().to_rfc3339();
            let _ = sqlx::query("UPDATE sessions SET native_session_id=?,status='idle',updated_at=? WHERE id=?")
                .bind(&result.native_session_id).bind(&now).bind(&session.id).execute(&db).await;
            if !result.assistant_text.trim().is_empty() {
                let _ = sqlx::query("INSERT INTO messages(id,session_id,role,content,created_at) VALUES(?,?,?,?,?)")
                    .bind(uuid::Uuid::new_v4().to_string()).bind(&session.id).bind("assistant")
                    .bind(&result.assistant_text).bind(&now).execute(&db).await;
            }
        }
        Err(error) => {
            let message = error.to_string();
            let status = if message == "agent process interrupted" { "interrupted" } else { "error" };
            let _ = sqlx::query("UPDATE sessions SET status=?,updated_at=? WHERE id=?")
                .bind(status).bind(Utc::now().to_rfc3339()).bind(&session.id).execute(&db).await;
            if status == "error" {
                events.publish(AgentEvent::Error { session_id: session.id, message });
            }
        }
    }
}
