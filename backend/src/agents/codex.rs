use super::{adapter::{AgentAdapter,AgentConfig,AgentRunResult},process::{ProcessAdapter,ProcessKind}};
use crate::events::EventBus;
use anyhow::Result;
use async_trait::async_trait;

pub struct CodexAdapter { process: ProcessAdapter }
impl CodexAdapter { pub fn new()->Self{Self{process:ProcessAdapter::new()}} }
#[async_trait]
impl AgentAdapter for CodexAdapter {
 async fn start(&self,config:&AgentConfig)->Result<()>{self.process.start(config).await}
 async fn send_message(&self,config:&AgentConfig,session_id:&str,message:&str,events:&EventBus)->Result<AgentRunResult>{self.process.run(ProcessKind::Codex,config,session_id,message,events).await}
 async fn interrupt(&self,session_id:&str)->Result<()>{self.process.interrupt_process(session_id).await}
}
