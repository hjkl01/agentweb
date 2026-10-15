use crate::events::EventBus;
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{error::Error, fmt};

use super::models::AgentModel;

#[derive(Debug, Clone)]
pub struct AgentRunError {
    pub interrupted: bool,
    pub native_session_id: Option<String>,
    pub native_session_missing: bool,
    message: String,
}

impl AgentRunError {
    pub fn interrupted(native_session_id: Option<String>) -> Self {
        Self {
            interrupted: true,
            native_session_id,
            native_session_missing: false,
            message: "agent process interrupted".into(),
        }
    }

    pub fn failed(message: impl Into<String>, native_session_id: Option<String>) -> Self {
        Self::failed_with_session_state(message, native_session_id, false)
    }

    pub fn missing_session(message: impl Into<String>, native_session_id: Option<String>) -> Self {
        Self::failed_with_session_state(message, native_session_id, true)
    }

    fn failed_with_session_state(
        message: impl Into<String>,
        native_session_id: Option<String>,
        native_session_missing: bool,
    ) -> Self {
        Self {
            interrupted: false,
            native_session_id,
            native_session_missing,
            message: message.into(),
        }
    }
}

impl fmt::Display for AgentRunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for AgentRunError {}

#[derive(Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub id: String,
    pub command: String,
    pub working_directory: Option<String>,
    pub native_session_id: Option<String>,
    pub fork_from_native_session_id: Option<String>,
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
        execution_id: &str,
        message: &str,
        events: &EventBus,
    ) -> Result<AgentRunResult>;

    async fn interrupt(&self, session_id: &str, execution_id: &str) -> Result<()>;

    async fn list_models(&self, config: &AgentConfig) -> Result<Vec<AgentModel>> {
        crate::agents::models::discover(&config.id, config).await
    }
}
