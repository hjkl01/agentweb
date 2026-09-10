use serde::Serialize;
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct EventBus(broadcast::Sender<AgentEvent>);

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
    ToolOutput {
        session_id: String,
        tool: String,
        output: String,
    },
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
    pub fn new() -> Self {
        Self(broadcast::channel(512).0)
    }
    pub fn publish(&self, event: AgentEvent) {
        let _ = self.0.send(event);
    }
    pub fn subscribe(&self) -> broadcast::Receiver<AgentEvent> {
        self.0.subscribe()
    }
}
