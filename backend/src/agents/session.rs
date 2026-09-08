use super::{adapter::AgentConfig, definition, AgentManager};
use crate::{api::Session, events::{AgentEvent, EventBus}};
use chrono::Utc;
use sqlx::Row;
use std::sync::Arc;

pub async fn run_session(
    db: sqlx::SqlitePool,
    agents: Arc<AgentManager>,
    session: Session,
    message: String,
    events: EventBus,
) {
    let row = sqlx::query("SELECT kind,command,working_directory,native_session_id,model FROM agents WHERE id=?")
        .bind(&session.agent_id)
        .fetch_optional(&db)
        .await
        .ok()
        .flatten();

    let (kind, command, working_directory, native_session_id, model) = if let Some(row) = row {
        (row.get(0), row.get(1), row.get(2), row.get(3), row.get(4))
    } else if let Some(def) = definition::BUILT_IN_AGENTS.iter().find(|a| a.id == session.agent_id) {
        (def.kind.to_owned(), def.command.to_owned(), None, session.native_session_id.clone(), session.model.clone())
    } else {
        events.publish(AgentEvent::Error { session_id: session.id, message: "agent not found".into() });
        return;
    };

    let config = AgentConfig {
        id: kind.clone(),
        command,
        working_directory: working_directory.or_else(|| Some(session.workspace.clone())),
        native_session_id: native_session_id.or(session.native_session_id.clone()),
        runtime_path: None,
        model: model.or(session.model.clone()),
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
            let _ = sqlx::query("UPDATE sessions SET status='error',updated_at=? WHERE id=?")
                .bind(Utc::now().to_rfc3339()).bind(&session.id).execute(&db).await;
            events.publish(AgentEvent::Error { session_id: session.id, message: error.to_string() });
        }
    }
}
