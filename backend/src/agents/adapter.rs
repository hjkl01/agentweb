use crate::events::EventBus;
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub id: String,
    pub command: String,
    pub working_directory: Option<String>,
    pub native_session_id: Option<String>,
    pub runtime_path: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct AgentRunResult {
    pub native_session_id: Option<String>,
    pub assistant_text: String,
}

#[async_trait]
pub trait AgentAdapter: Send + Sync {
    async fn start(&self, config: &AgentConfig) -> Result<()>;
    async fn send_message(
        &self,
        config: &AgentConfig,
        session_id: &str,
        message: &str,
        events: &EventBus,
    ) -> Result<AgentRunResult>;
    async fn interrupt(&self, session_id: &str) -> Result<()>;
}
