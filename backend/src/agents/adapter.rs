use crate::events::EventBus;
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::models::AgentModel;

#[derive(Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub id: String,
    pub command: String,
    pub working_directory: Option<String>,
    pub native_session_id: Option<String>,
    pub runtime_path: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct AgentRunResult {
    pub native_session_id: Option<String>,
    pub assistant_text: String,
}

#[async_trait]
pub trait AgentAdapter: Send + Sync {
    async fn send_message(
        &self,
        config: &AgentConfig,
        session_id: &str,
        message: &str,
        events: &EventBus,
    ) -> Result<AgentRunResult>;

    async fn interrupt(&self, session_id: &str) -> Result<()>;

    async fn list_models(&self, config: &AgentConfig) -> Result<Vec<AgentModel>> {
        crate::agents::models::discover(&config.id, config).await
    }
}
