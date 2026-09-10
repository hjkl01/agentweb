use chrono::Utc;
use serde::Serialize;
use sqlx::SqlitePool;
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct EventBus {
    sender: broadcast::Sender<AgentEvent>,
    db: SqlitePool,
    worker_id: Option<String>,
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

impl EventBus {
    pub fn new(db: SqlitePool) -> Self {
        Self { sender: broadcast::channel(512).0, db, worker_id: None }
    }

    pub fn for_worker(&self, worker_id: String) -> Self {
        Self { sender: self.sender.clone(), db: self.db.clone(), worker_id: Some(worker_id) }
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
        let worker_id = self.worker_id.clone();
        let db = self.db.clone();
        tokio::spawn(async move {
            if let Some(session_id) = session_id {
                // The worker may still emit events while another worker deletes the session.
                // Guard the write so a late event cannot create an orphan row.
                let result = sqlx::query("INSERT INTO agent_events(session_id,worker_id,event_type,payload,created_at) SELECT ?,?,?,?,? WHERE EXISTS (SELECT 1 FROM sessions WHERE id=?)")
                    .bind(&session_id)
                    .bind(worker_id)
                    .bind(event_type)
                    .bind(payload)
                    .bind(Utc::now().to_rfc3339())
                    .bind(&session_id)
                    .execute(&db)
                    .await;
                if let Err(error) = result {
                    tracing::warn!(%error, "failed to persist agent event");
                }
            }
        });
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AgentEvent> {
        self.sender.subscribe()
    }
}
