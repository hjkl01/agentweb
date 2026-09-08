use super::{
    adapter::{AgentAdapter, AgentConfig, AgentRunResult},
    commands::ProcessKind,
    process::ProcessAdapter,
};
use crate::events::EventBus;
use anyhow::Result;
use async_trait::async_trait;

pub struct PiAdapter {
    process: ProcessAdapter,
}
impl PiAdapter {
    pub fn new() -> Self {
        Self {
            process: ProcessAdapter::new(),
        }
    }
}
#[async_trait]
impl AgentAdapter for PiAdapter {
    async fn send_message(
        &self,
        config: &AgentConfig,
        session_id: &str,
        message: &str,
        events: &EventBus,
    ) -> Result<AgentRunResult> {
        self.process
            .run(ProcessKind::Pi, config, session_id, message, events)
            .await
    }
    async fn interrupt(&self, session_id: &str) -> Result<()> {
        self.process.interrupt_process(session_id).await
    }
}
