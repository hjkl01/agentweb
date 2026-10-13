use super::{
    adapter::{AgentAdapter, AgentConfig, AgentRunResult},
    commands::ProcessKind,
    process::ProcessAdapter,
};
use crate::events::EventBus;
use anyhow::Result;
use async_trait::async_trait;

pub struct CodexAdapter {
    process: ProcessAdapter,
}
impl CodexAdapter {
    pub fn new() -> Self {
        Self {
            process: ProcessAdapter::new(),
        }
    }
}
#[async_trait]
impl AgentAdapter for CodexAdapter {
    async fn send_message(
        &self,
        config: &AgentConfig,
        session_id: &str,
        execution_id: &str,
        message: &str,
        events: &EventBus,
    ) -> Result<AgentRunResult> {
        self.process
            .run_for_execution(ProcessKind::Codex, config, session_id, execution_id, message, events)
            .await
    }
    async fn interrupt(&self, session_id: &str, execution_id: &str) -> Result<()> {
        self.process.interrupt_execution(session_id, execution_id).await
    }
}
