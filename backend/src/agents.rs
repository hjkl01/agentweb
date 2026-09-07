use crate::events::{AgentEvent, EventBus};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
use tokio::{process::Command, sync::Mutex};

#[derive(Clone, Serialize, Deserialize)]
pub struct AgentConfig { pub id: String, pub command: String, pub working_directory: Option<String> }

#[async_trait]
pub trait AgentAdapter: Send + Sync {
    async fn start(&self, config: &AgentConfig) -> Result<()>;
    async fn send_message(&self, config: &AgentConfig, session_id: &str, message: &str, events: &EventBus) -> Result<()>;
    async fn interrupt(&self, session_id: &str) -> Result<()>;
}

pub struct ProcessAgent;

#[async_trait]
impl AgentAdapter for ProcessAgent {
    async fn start(&self, _config: &AgentConfig) -> Result<()> { Ok(()) }
    async fn send_message(&self, config: &AgentConfig, session_id: &str, message: &str, events: &EventBus) -> Result<()> {
        let mut parts = config.command.split_whitespace();
        let program = parts.next().ok_or_else(|| anyhow!("empty agent command"))?;
        let mut cmd = Command::new(program);
        cmd.args(parts).arg(message);
        if let Some(dir) = &config.working_directory { cmd.current_dir(dir); }
        let output = cmd.output().await?;
        if !output.stdout.is_empty() { events.publish(AgentEvent::MessageDelta { session_id: session_id.into(), text: String::from_utf8_lossy(&output.stdout).into_owned() }); }
        if !output.stderr.is_empty() { events.publish(AgentEvent::Error { session_id: session_id.into(), message: String::from_utf8_lossy(&output.stderr).into_owned() }); }
        events.publish(AgentEvent::MessageCompleted { session_id: session_id.into() });
        Ok(())
    }
    async fn interrupt(&self, _session_id: &str) -> Result<()> { Ok(()) }
}

#[derive(Default)]
pub struct AgentManager { adapters: Mutex<HashMap<String, Arc<dyn AgentAdapter>>> }
impl AgentManager {
    pub fn new() -> Self { Self::default() }
    pub async fn adapter(&self, kind: &str) -> Arc<dyn AgentAdapter> {
        let mut m = self.adapters.lock().await;
        m.entry(kind.into()).or_insert_with(|| Arc::new(ProcessAgent)).clone()
    }
}
