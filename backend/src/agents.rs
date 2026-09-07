use crate::events::{AgentEvent, EventBus};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::Mutex,
};

#[derive(Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub id: String,
    pub command: String,
    pub working_directory: Option<String>,
}

#[async_trait]
pub trait AgentAdapter: Send + Sync {
    async fn start(&self, config: &AgentConfig) -> Result<()>;
    async fn send_message(&self, config: &AgentConfig, session_id: &str, message: &str, events: &EventBus) -> Result<()>;
    async fn interrupt(&self, session_id: &str) -> Result<()>;
}

#[derive(Default)]
pub struct ProcessAgent {
    processes: Mutex<HashMap<String, Arc<Mutex<Child>>>>,
}

impl ProcessAgent {
    fn command_parts(command: &str) -> Result<(String, Vec<String>)> {
        let mut parts = command.split_whitespace();
        let program = parts.next().ok_or_else(|| anyhow!("empty agent command"))?.to_string();
        Ok((program, parts.map(str::to_string).collect()))
    }
}

#[async_trait]
impl AgentAdapter for ProcessAgent {
    async fn start(&self, _config: &AgentConfig) -> Result<()> { Ok(()) }

    async fn send_message(&self, config: &AgentConfig, session_id: &str, message: &str, events: &EventBus) -> Result<()> {
        if self.processes.lock().await.contains_key(session_id) {
            return Err(anyhow!("session already has a running agent process"));
        }

        let (program, args) = Self::command_parts(&config.command)?;
        let mut command = Command::new(program);
        command.args(args).arg(message);
        if let Some(dir) = &config.working_directory { command.current_dir(dir); }
        command.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());

        let mut child = command.spawn()?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let child = Arc::new(Mutex::new(child));
        self.processes.lock().await.insert(session_id.to_string(), child.clone());
        events.publish(AgentEvent::MessageStarted { session_id: session_id.to_string() });

        let sid = session_id.to_string();
        let stdout_task = tokio::spawn(async move {
            let mut output = String::new();
            if let Some(stdout) = stdout {
                let mut lines = BufReader::new(stdout).lines();
                while let Some(line) = lines.next_line().await? {
                    let text = format!("{line}\n");
                    output.push_str(&text);
                }
            }
            Ok::<String, anyhow::Error>(output)
        });

        let sid_err = session_id.to_string();
        let stderr_task = tokio::spawn(async move {
            let mut errors = Vec::new();
            if let Some(stderr) = stderr {
                let mut lines = BufReader::new(stderr).lines();
                while let Some(line) = lines.next_line().await? {
                    errors.push(line);
                }
            }
            Ok::<Vec<String>, anyhow::Error>(errors)
        });

        let status = child.lock().await.wait().await?;
        let stdout_result = stdout_task.await??;
        let stderr_result = stderr_task.await??;

        for line in stdout_result.lines() {
            events.publish(AgentEvent::MessageDelta { session_id: sid.clone(), text: format!("{line}\n") });
        }
        for line in stderr_result {
            events.publish(AgentEvent::Error { session_id: sid_err.clone(), message: line });
        }

        self.processes.lock().await.remove(session_id);
        if status.success() {
            events.publish(AgentEvent::MessageCompleted { session_id: session_id.to_string() });
            events.publish(AgentEvent::SessionCompleted { session_id: session_id.to_string() });
        } else {
            events.publish(AgentEvent::Error { session_id: session_id.to_string(), message: format!("agent exited with status {status}") });
        }
        Ok(())
    }

    async fn interrupt(&self, session_id: &str) -> Result<()> {
        if let Some(child) = self.processes.lock().await.remove(session_id) {
            child.lock().await.kill().await?;
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct AgentManager {
    adapters: Mutex<HashMap<String, Arc<ProcessAgent>>>,
}

impl AgentManager {
    pub fn new() -> Self { Self::default() }

    pub async fn adapter(&self, kind: &str) -> Arc<ProcessAgent> {
        let mut map = self.adapters.lock().await;
        map.entry(kind.to_string()).or_insert_with(|| Arc::new(ProcessAgent::default())).clone()
    }
}
