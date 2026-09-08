use super::{adapter::AgentRunError, definition, AgentManager};
use crate::{api::{build_agent_config, Session}, events::{AgentEvent, EventBus}};
use chrono::Utc;
use sqlx::Row;
use std::sync::Arc;

async fn finish_session(db: &sqlx::SqlitePool, session_id: &str, status: &str) {
    let _ = sqlx::query("UPDATE sessions SET status=?,updated_at=? WHERE id=? AND status='running'")
        .bind(status).bind(Utc::now().to_rfc3339()).bind(session_id).execute(db).await;
}

pub async fn run_session(db: sqlx::SqlitePool, agents: Arc<AgentManager>, session: Session, message: String, events: EventBus) {
    let mut config = match build_agent_config(&db, &session.agent_id).await {
        Ok(config) => config,
        Err(_) => {
            finish_session(&db, &session.id, "error").await;
            events.publish(AgentEvent::Error { session_id: session.id, message: "agent executable not found".into() });
            return;
        }
    };
    config.working_directory = Some(session.workspace.clone());
    config.native_session_id = session.native_session_id.clone();
    config.model = session.model.clone();
    let kind = config.id.clone();
    let adapter = agents.adapter(&kind).await;
    match adapter.send_message(&config, &session.id, &message, &events).await {
        Ok(result) => {
            let now = Utc::now().to_rfc3339();
            let updated = sqlx::query("UPDATE sessions SET native_session_id=?,status='idle',updated_at=? WHERE id=? AND status='running'")
                .bind(&result.native_session_id).bind(&now).bind(&session.id).execute(&db).await;
            if updated.map(|r| r.rows_affected() == 1).unwrap_or(false) && !result.assistant_text.trim().is_empty() {
                let _ = sqlx::query("INSERT INTO messages(id,session_id,role,content,created_at) VALUES(?,?,?,?,?)")
                    .bind(uuid::Uuid::new_v4().to_string()).bind(&session.id).bind("assistant")
                    .bind(&result.assistant_text).bind(&now).execute(&db).await;
            }
        }
        Err(error) => {
            let interrupted = error.downcast_ref::<AgentRunError>().is_some();
            finish_session(&db, &session.id, if interrupted { "interrupted" } else { "error" }).await;
            if !interrupted { events.publish(AgentEvent::Error { session_id: session.id, message: error.to_string() }); }
        }
    }
}
