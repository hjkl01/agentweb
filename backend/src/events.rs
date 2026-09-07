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
    pub fn new() -> Self { Self(broadcast::channel(512).0) }
    pub fn publish(&self, event: AgentEvent) { let _ = self.0.send(event); }
    pub fn subscribe(&self) -> broadcast::Receiver<AgentEvent> { self.0.subscribe() }
}
