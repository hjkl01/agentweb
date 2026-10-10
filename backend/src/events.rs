use chrono::Utc;
use serde::Serialize;
use sqlx::SqlitePool;
use tokio::sync::{broadcast, mpsc};

#[derive(Clone)]
pub struct EventBus {
    sender: broadcast::Sender<AgentEvent>,
    persist_sender: mpsc::UnboundedSender<PersistedEvent>,
    worker_id: Option<String>,
}

struct PersistedEvent {
    session_id: Option<String>,
    worker_id: Option<String>,
    event_type: String,
    payload: String,
    created_at: String,
}

#[derive(Clone, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum AgentEvent {
    #[serde(rename = "session.started")]
    SessionStarted { session_id: String },
    #[serde(rename = "message.started")]
    MessageStarted { session_id: String },
    #[serde(rename = "message.delta")]
    MessageDelta { session_id: String, text: String },
    #[serde(rename = "message.completed")]
    MessageCompleted { session_id: String },
    #[serde(rename = "thinking.started")]
    ThinkingStarted { session_id: String },
    #[serde(rename = "thinking.delta")]
    ThinkingDelta { session_id: String, text: String },
    #[serde(rename = "thinking.completed")]
    ThinkingCompleted { session_id: String },
    #[serde(rename = "tool.started")]
    ToolStarted { session_id: String, tool: String },
    #[serde(rename = "tool.output")]
    ToolOutput { session_id: String, tool: String, output: String },
    #[serde(rename = "tool.completed")]
    ToolCompleted { session_id: String, tool: String },
    #[serde(rename = "file.created")]
    FileCreated { session_id: String, path: String },
    #[serde(rename = "file.modified")]
    FileModified { session_id: String, path: String },
    #[serde(rename = "file.deleted")]
    FileDeleted { session_id: String, path: String },
    #[serde(rename = "command.started")]
    CommandStarted { session_id: String, command: String },
    #[serde(rename = "command.output")]
    CommandOutput { session_id: String, output: String },
    #[serde(rename = "command.completed")]
    CommandCompleted { session_id: String },
    #[serde(rename = "agent.error")]
    Error { session_id: String, message: String },
    #[serde(rename = "session.completed")]
    SessionCompleted { session_id: String },
    #[serde(rename = "install.output")]
    InstallOutput { agent_id: String, text: String },
    #[serde(rename = "install.completed")]
    InstallCompleted { agent_id: String, success: bool },
}

// Returns whether the failed operation is safe to retry. A failed commit is
// deliberately not retried: the database may have committed despite the error.
async fn persist_batch(
    db: &SqlitePool,
    batch: &[PersistedEvent],
) -> Result<(), (sqlx::Error, bool)> {
    let mut transaction = db.begin().await.map_err(|error| (error, true))?;
    for event in batch {
        if let Err(error) = sqlx::query(
            "INSERT INTO agent_events(session_id,worker_id,event_type,payload,created_at)
             SELECT ?,?,?,?,?
             WHERE ? IS NULL OR EXISTS (SELECT 1 FROM sessions WHERE id=?)",
        )
        .bind(&event.session_id)
        .bind(&event.worker_id)
        .bind(&event.event_type)
        .bind(&event.payload)
        .bind(&event.created_at)
        .bind(event.session_id.as_deref())
        .bind(event.session_id.as_deref())
        .execute(&mut *transaction)
        .await
        {
            if let Err(rollback_error) = transaction.rollback().await {
                tracing::warn!(%rollback_error, "failed to roll back agent event batch");
            }
            return Err((error, true));
        }
    }
    transaction.commit().await.map_err(|error| (error, false))
}

impl EventBus {
    pub fn new(db: SqlitePool) -> Self {
        let (persist_sender, mut persist_receiver) = mpsc::unbounded_channel::<PersistedEvent>();
        tokio::spawn(async move {
            // Batch nearby events into one transaction while preserving publication order.
            // The short deadline keeps low-volume events responsive; the size limit bounds
            // transaction duration during high-volume token streaming.
            while let Some(first) = persist_receiver.recv().await {
                let mut batch = Vec::with_capacity(100);
                batch.push(first);
                let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(5);
                while batch.len() < 100 {
                    match tokio::time::timeout_at(deadline, persist_receiver.recv()).await {
                        Ok(Some(event)) => batch.push(event),
                        Ok(None) | Err(_) => break,
                    }
                }

                let mut attempt = 0_u32;
                loop {
                    match persist_batch(&db, &batch).await {
                        Ok(()) => break,
                        Err((error, retryable)) if retryable && attempt < 5 => {
                            let delay_ms = 25_u64 * (1_u64 << attempt);
                            attempt += 1;
                            tracing::warn!(
                                %error,
                                attempt,
                                delay_ms,
                                "agent event batch persistence failed; retrying"
                            );
                            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        }
                        Err((error, retryable)) => {
                            if retryable {
                                tracing::error!(
                                    %error,
                                    attempts = attempt + 1,
                                    "dropping agent event batch after persistence retries were exhausted"
                                );
                            } else {
                                tracing::error!(
                                    %error,
                                    "agent event batch commit failed; not retrying because commit outcome may be ambiguous"
                                );
                            }
                            break;
                        }
                    }
                }
            }
        });
        Self { sender: broadcast::channel(512).0, persist_sender, worker_id: None }
    }

    pub fn for_worker(&self, worker_id: String) -> Self {
        Self { sender: self.sender.clone(), persist_sender: self.persist_sender.clone(), worker_id: Some(worker_id) }
    }

    pub fn publish(&self, event: AgentEvent) {
        let _ = self.sender.send(event.clone());
        let Ok(payload) = serde_json::to_string(&event) else { return; };
        let session_id = match &event {
            AgentEvent::SessionStarted { session_id }
            | AgentEvent::MessageStarted { session_id }
            | AgentEvent::MessageDelta { session_id, .. }
            | AgentEvent::MessageCompleted { session_id }
            | AgentEvent::ThinkingStarted { session_id }
            | AgentEvent::ThinkingDelta { session_id, .. }
            | AgentEvent::ThinkingCompleted { session_id }
            | AgentEvent::ToolStarted { session_id, .. }
            | AgentEvent::ToolOutput { session_id, .. }
            | AgentEvent::ToolCompleted { session_id, .. }
            | AgentEvent::FileCreated { session_id, .. }
            | AgentEvent::FileModified { session_id, .. }
            | AgentEvent::FileDeleted { session_id, .. }
            | AgentEvent::CommandStarted { session_id, .. }
            | AgentEvent::CommandOutput { session_id, .. }
            | AgentEvent::CommandCompleted { session_id }
            | AgentEvent::Error { session_id, .. }
            | AgentEvent::SessionCompleted { session_id } => Some(session_id.clone()),
            AgentEvent::InstallOutput { .. } | AgentEvent::InstallCompleted { .. } => None,
        };
        let event_type = serde_json::from_str::<serde_json::Value>(&payload)
            .ok()
            .and_then(|value| value.get("type").and_then(|value| value.as_str()).map(str::to_owned))
            .unwrap_or_default();
        if self.persist_sender.send(PersistedEvent {
            session_id,
            worker_id: self.worker_id.clone(),
            event_type,
            payload,
            created_at: Utc::now().to_rfc3339(),
        }).is_err() {
            tracing::warn!("agent event persistence worker is unavailable");
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AgentEvent> {
        self.sender.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn bus() -> (EventBus, SqlitePool) {
        let db = SqlitePoolOptions::new().max_connections(1).idle_timeout(None).connect("sqlite::memory:").await.unwrap();
        sqlx::query("CREATE TABLE sessions(id TEXT PRIMARY KEY)").execute(&db).await.unwrap();
        sqlx::query("CREATE TABLE agent_events(id INTEGER PRIMARY KEY AUTOINCREMENT,session_id TEXT,worker_id TEXT,event_type TEXT NOT NULL,payload TEXT NOT NULL,created_at TEXT NOT NULL)").execute(&db).await.unwrap();
        (EventBus::new(db.clone()), db)
    }

    async fn rows(db: &SqlitePool) -> Vec<(Option<String>, String)> {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        sqlx::query_as("SELECT session_id,event_type FROM agent_events ORDER BY id").fetch_all(db).await.unwrap()
    }

    #[tokio::test]
    async fn persists_install_events_in_publication_order_and_guards_missing_sessions() {
        let (bus, db) = bus().await;
        bus.publish(AgentEvent::InstallOutput { agent_id: "pi".into(), text: "Installing pi …".into() });
        bus.publish(AgentEvent::InstallCompleted { agent_id: "pi".into(), success: true });
        bus.publish(AgentEvent::MessageDelta { session_id: "gone".into(), text: "x".into() });
        assert_eq!(
            rows(&db).await,
            [(None, "install.output".to_owned()), (None, "install.completed".to_owned())]
        );

        sqlx::query("INSERT INTO sessions(id) VALUES('live')").execute(&db).await.unwrap();
        bus.publish(AgentEvent::MessageStarted { session_id: "live".into() });
        bus.publish(AgentEvent::MessageDelta { session_id: "live".into(), text: "x".into() });
        bus.publish(AgentEvent::MessageCompleted { session_id: "live".into() });
        assert_eq!(
            rows(&db).await,
            [
                (None, "install.output".to_owned()),
                (None, "install.completed".to_owned()),
                (Some("live".to_owned()), "message.started".to_owned()),
                (Some("live".to_owned()), "message.delta".to_owned()),
                (Some("live".to_owned()), "message.completed".to_owned()),
            ]
        );
    }
}
