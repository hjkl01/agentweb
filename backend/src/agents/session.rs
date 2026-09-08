use super::{adapter::AgentRunError, AgentManager};
use crate::{api::{build_agent_config, Session}, events::{AgentEvent, EventBus}};
use chrono::Utc;
use sqlx::Row;
use std::sync::Arc;
use tokio::time::{self, Duration};

async fn finish_session(db: &sqlx::SqlitePool, session_id: &str, status: &str) {
    let _ = sqlx::query("UPDATE sessions SET status=?,updated_at=? WHERE id=? AND status='running'")
        .bind(status).bind(Utc::now().to_rfc3339()).bind(session_id).execute(db).await;
}

async fn persist_stream(db: sqlx::SqlitePool, session_id: String, message_id: String, events: EventBus) {
    let mut rx = events.subscribe();
    let mut pending = String::new();
    let mut ticker = time::interval(Duration::from_millis(100));
    loop {
        tokio::select! {
            result = rx.recv() => match result {
                Ok(AgentEvent::MessageDelta { session_id: id, text }) if id == session_id => pending.push_str(&text),
                Ok(AgentEvent::MessageCompleted { session_id: id }) | Ok(AgentEvent::SessionCompleted { session_id: id }) if id == session_id => {
                    if !pending.is_empty() {
                        let _ = sqlx::query("UPDATE messages SET content=content||? WHERE id=?")
                            .bind(&pending).bind(&message_id).execute(&db).await;
                        pending.clear();
                    }
                    return;
                }
                Ok(_) => {}
                Err(_) => return,
            },
            _ = ticker.tick() => {
                if !pending.is_empty() {
                    let chunk = std::mem::take(&mut pending);
                    if sqlx::query("UPDATE messages SET content=content||? WHERE id=?")
                        .bind(&chunk).bind(&message_id).execute(&db).await.is_err() { return; }
                }
            }
        }
    }
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
    let assistant_id = uuid::Uuid::new_v4().to_string();
    let created_at = Utc::now().to_rfc3339();
    if sqlx::query("INSERT INTO messages(id,session_id,role,content,created_at) VALUES(?,?,?,?,?)")
        .bind(&assistant_id).bind(&session.id).bind("assistant").bind("").bind(&created_at).execute(&db).await.is_err() {
        finish_session(&db, &session.id, "error").await;
        events.publish(AgentEvent::Error { session_id: session.id, message: "failed to create assistant message".into() });
        return;
    }
    let persist_task = tokio::spawn(persist_stream(db.clone(), session.id.clone(), assistant_id.clone(), events.clone()));
    let adapter = agents.adapter(&kind).await;
    match adapter.send_message(&config, &session.id, &message, &events).await {
        Ok(result) => {
            persist_task.abort();
            let now = Utc::now().to_rfc3339();
            let updated = sqlx::query("UPDATE sessions SET native_session_id=?,status='idle',updated_at=? WHERE id=? AND status='running'")
                .bind(&result.native_session_id).bind(&now).bind(&session.id).execute(&db).await;
            if updated.map(|r| r.rows_affected() == 1).unwrap_or(false) {
                let _ = sqlx::query("UPDATE messages SET content=? WHERE id=?")
                    .bind(&result.assistant_text).bind(&assistant_id).execute(&db).await;
            }
        }
        Err(error) => {
            persist_task.abort();
            let interrupted = error.downcast_ref::<AgentRunError>().is_some();
            finish_session(&db, &session.id, if interrupted { "interrupted" } else { "error" }).await;
            let content: Option<String> = sqlx::query_scalar("SELECT content FROM messages WHERE id=?").bind(&assistant_id).fetch_optional(&db).await.unwrap_or(None);
            if content.as_deref().map(str::trim).unwrap_or_default().is_empty() {
                let _ = sqlx::query("DELETE FROM messages WHERE id=?").bind(&assistant_id).execute(&db).await;
            }
            if !interrupted { events.publish(AgentEvent::Error { session_id: session.id, message: error.to_string() }); }
        }
    }
}
