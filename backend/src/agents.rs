use crate::events::{AgentEvent, EventBus};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};
use tokio::{io::{AsyncBufReadExt, BufReader}, process::{Child, Command}, sync::Mutex};

#[derive(Clone, Serialize, Deserialize)]
pub struct AgentConfig { pub id: String, pub command: String, pub working_directory: Option<String> }

#[async_trait]
pub trait AgentAdapter: Send + Sync {
    async fn start(&self, config: &AgentConfig) -> Result<()>;
    async fn send_message(&self, config: &AgentConfig, session_id: &str, message: &str, events: &EventBus) -> Result<()>;
    async fn interrupt(&self, session_id: &str) -> Result<()>;
}

#[derive(Default)]
pub struct ProcessAgent { processes: Mutex<HashMap<String, Arc<Mutex<Child>>>> }

impl ProcessAgent {
    fn command_parts(command: &str) -> Result<(String, Vec<String>)> {
        let mut parts = command.split_whitespace();
        let program = parts.next().ok_or_else(|| anyhow!("empty agent command"))?.to_string();
        Ok((program, parts.map(str::to_string).collect()))
    }

    fn emit_stdout_line(events: &EventBus, session_id: &str, line: &str) {
        if let Ok(value) = serde_json::from_str::<Value>(line) {
            let typ = value.get("type").and_then(Value::as_str).unwrap_or("");
            match typ {
                "agent_message_delta" => {
                    if let Some(text) = value.get("delta").and_then(Value::as_str) {
                        events.publish(AgentEvent::MessageDelta { session_id: session_id.into(), text: text.into() });
                    }
                }
                "agent_message" => {
                    if let Some(text) = value.get("message").and_then(Value::as_str).or_else(|| value.get("text").and_then(Value::as_str)) {
                        events.publish(AgentEvent::MessageDelta { session_id: session_id.into(), text: text.into() });
                    }
                }
                "item.completed" => {
                    let item = value.get("item").unwrap_or(&Value::Null);
                    if item.get("type").and_then(Value::as_str) == Some("agent_message") {
                        if let Some(text) = item.get("text").and_then(Value::as_str) {
                            events.publish(AgentEvent::MessageDelta { session_id: session_id.into(), text: text.into() });
                        }
                    }
                }
                "error" | "turn.failed" => {
                    let text = value.get("message").and_then(Value::as_str)
                        .or_else(|| value.get("error").and_then(|v| v.get("message")).and_then(Value::as_str))
                        .unwrap_or(line);
                    events.publish(AgentEvent::Error { session_id: session_id.into(), message: text.into() });
                }
                _ => {}
            }
        } else if !line.trim().is_empty() {
            events.publish(AgentEvent::MessageDelta { session_id: session_id.into(), text: format!("{line}\n") });
        }
    }
}

#[async_trait]
impl AgentAdapter for ProcessAgent {
    async fn start(&self, _config: &AgentConfig) -> Result<()> { Ok(()) }

    async fn send_message(&self, config: &AgentConfig, session_id: &str, message: &str, events: &EventBus) -> Result<()> {
        if self.processes.lock().await.contains_key(session_id) { return Err(anyhow!("session already has a running agent process")); }
        let (program, args) = Self::command_parts(&config.command)?;
        let mut command = Command::new(program);
        command.args(args).arg(message);
        if let Some(dir) = &config.working_directory { command.current_dir(dir); }
        command.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
        let mut child = command.spawn()?;
        let stdout = child.stdout.take(); let stderr = child.stderr.take();
        let child = Arc::new(Mutex::new(child));
        self.processes.lock().await.insert(session_id.to_string(), child.clone());
        events.publish(AgentEvent::MessageStarted { session_id: session_id.into() });

        let sid = session_id.to_string(); let out_events = events.clone();
        let stdout_task = tokio::spawn(async move {
            if let Some(stdout) = stdout { let mut lines = BufReader::new(stdout).lines(); while let Some(line) = lines.next_line().await? { ProcessAgent::emit_stdout_line(&out_events, &sid, &line); } }
            Ok::<(), anyhow::Error>(())
        });
        let sid = session_id.to_string(); let err_events = events.clone();
        let stderr_task = tokio::spawn(async move {
            if let Some(stderr) = stderr { let mut lines = BufReader::new(stderr).lines(); while let Some(line) = lines.next_line().await? { err_events.publish(AgentEvent::Error { session_id: sid.clone(), message: line }); } }
            Ok::<(), anyhow::Error>(())
        });
        let status = child.lock().await.wait().await?;
        stdout_task.await??; stderr_task.await??;
        self.processes.lock().await.remove(session_id);
        if status.success() { events.publish(AgentEvent::MessageCompleted { session_id: session_id.into() }); events.publish(AgentEvent::SessionCompleted { session_id: session_id.into() }); }
        else { events.publish(AgentEvent::Error { session_id: session_id.into(), message: format!("agent exited with status {status}") }); }
        Ok(())
    }

    async fn interrupt(&self, session_id: &str) -> Result<()> {
        if let Some(child) = self.processes.lock().await.remove(session_id) { child.lock().await.kill().await?; }
        Ok(())
    }
}

#[derive(Default)]
pub struct AgentManager { adapters: Mutex<HashMap<String, Arc<ProcessAgent>>> }
impl AgentManager {
    pub fn new() -> Self { Self::default() }
    pub async fn adapter(&self, kind: &str) -> Arc<ProcessAgent> { let mut map = self.adapters.lock().await; map.entry(kind.to_string()).or_insert_with(|| Arc::new(ProcessAgent::default())).clone() }
}
