use crate::events::{AgentEvent, EventBus};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};
use tokio::{io::{AsyncBufReadExt, BufReader}, process::{Child, Command}, sync::Mutex};

#[derive(Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub id: String,
    pub command: String,
    pub working_directory: Option<String>,
    pub native_session_id: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct AgentRunResult {
    pub native_session_id: Option<String>,
    pub assistant_text: String,
}

#[async_trait]
pub trait AgentAdapter: Send + Sync {
    async fn start(&self, config: &AgentConfig) -> Result<()>;
    async fn send_message(&self, config: &AgentConfig, session_id: &str, message: &str, events: &EventBus) -> Result<AgentRunResult>;
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

    fn build_command(config: &AgentConfig, message: &str) -> Result<Command> {
        let (program, mut args) = Self::command_parts(&config.command)?;
        let mut command = Command::new(program);
        match config.id.as_str() {
            // Codex keeps conversation state in a persisted thread. The first turn creates
            // the thread; subsequent turns use the exact native thread id.
            "codex" => {
                if let Some(thread) = &config.native_session_id {
                    args.extend(["resume".into(), thread.clone()]);
                }
                args.push("--json".into());
                args.push(message.into());
            }
            // OpenCode exposes the same concept as a session id for `run`.
            "opencode" => {
                args.push("run".into());
                if let Some(session) = &config.native_session_id {
                    args.extend(["--session".into(), session.clone()]);
                }
                args.extend(["--format".into(), "json".into(), message.into()]);
            }
            // Pi supports a stable session path/id and JSON event mode.
            "pi" => {
                args.extend(["--mode".into(), "json".into()]);
                if let Some(session) = &config.native_session_id {
                    args.extend(["--session".into(), session.clone()]);
                }
                args.extend(["-p".into(), message.into()]);
            }
            // Generic installed/custom agents remain one-shot until their adapter is known.
            _ => args.push(message.into()),
        }
        command.args(args);
        if let Some(dir) = &config.working_directory { command.current_dir(dir); }
        command.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
        Ok(command)
    }

    fn parse_event(value: &Value) -> (Option<String>, Option<String>, Option<String>) {
        let typ = value.get("type").and_then(Value::as_str).unwrap_or("");
        let session = value.get("thread_id").and_then(Value::as_str)
            .or_else(|| value.get("session_id").and_then(Value::as_str))
            .or_else(|| value.get("sessionId").and_then(Value::as_str))
            .map(str::to_string);
        let text = value.get("delta").and_then(Value::as_str)
            .or_else(|| value.get("text").and_then(Value::as_str))
            .or_else(|| value.get("message").and_then(Value::as_str))
            .or_else(|| value.get("item").and_then(|v| v.get("text")).and_then(Value::as_str))
            .map(str::to_string);
        (Some(typ.to_string()), session, text)
    }
}

#[async_trait]
impl AgentAdapter for ProcessAgent {
    async fn start(&self, _config: &AgentConfig) -> Result<()> { Ok(()) }

    async fn send_message(&self, config: &AgentConfig, session_id: &str, message: &str, events: &EventBus) -> Result<AgentRunResult> {
        if self.processes.lock().await.contains_key(session_id) { return Err(anyhow!("session already has a running agent process")); }
        let mut command = Self::build_command(config, message)?;
        let mut child = command.spawn()?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let child = Arc::new(Mutex::new(child));
        self.processes.lock().await.insert(session_id.to_string(), child.clone());
        events.publish(AgentEvent::MessageStarted { session_id: session_id.into() });

        let result = Arc::new(Mutex::new(AgentRunResult::default()));
        let sid = session_id.to_string();
        let out_events = events.clone();
        let out_result = result.clone();
        let stdout_task = tokio::spawn(async move {
            if let Some(stdout) = stdout {
                let mut lines = BufReader::new(stdout).lines();
                while let Some(line) = lines.next_line().await? {
                    if let Ok(value) = serde_json::from_str::<Value>(&line) {
                        let (typ, native, text) = ProcessAgent::parse_event(&value);
                        if let Some(native) = native { out_result.lock().await.native_session_id = Some(native); }
                        if let Some(text) = text {
                            if matches!(typ.as_deref(), Some("agent_message_delta") | Some("agent_message") | Some("item.completed") | Some("message_update") | Some("text_delta") | Some("assistant")) {
                                out_result.lock().await.assistant_text.push_str(&text);
                                out_events.publish(AgentEvent::MessageDelta { session_id: sid.clone(), text });
                            }
                        }
                        if matches!(typ.as_deref(), Some("error") | Some("turn.failed")) {
                            let error = value.get("message").and_then(Value::as_str)
                                .or_else(|| value.get("error").and_then(|v| v.get("message")).and_then(Value::as_str))
                                .unwrap_or(&line);
                            out_events.publish(AgentEvent::Error { session_id: sid.clone(), message: error.to_string() });
                        }
                    } else if !line.trim().is_empty() {
                        out_result.lock().await.assistant_text.push_str(&format!("{line}\n"));
                        out_events.publish(AgentEvent::MessageDelta { session_id: sid.clone(), text: format!("{line}\n") });
                    }
                }
            }
            Ok::<(), anyhow::Error>(())
        });

        let sid = session_id.to_string();
        let err_events = events.clone();
        let stderr_task = tokio::spawn(async move {
            if let Some(stderr) = stderr {
                let mut lines = BufReader::new(stderr).lines();
                while let Some(line) = lines.next_line().await? {
                    err_events.publish(AgentEvent::Error { session_id: sid.clone(), message: line });
                }
            }
            Ok::<(), anyhow::Error>(())
        });

        let status = child.lock().await.wait().await?;
        stdout_task.await??;
        stderr_task.await??;
        self.processes.lock().await.remove(session_id);
        let result = result.lock().await.clone();
        if status.success() {
            events.publish(AgentEvent::MessageCompleted { session_id: session_id.into() });
            events.publish(AgentEvent::SessionCompleted { session_id: session_id.into() });
        } else {
            events.publish(AgentEvent::Error { session_id: session_id.into(), message: format!("agent exited with status {status}") });
        }
        Ok(result)
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
    pub async fn adapter(&self, kind: &str) -> Arc<ProcessAgent> {
        let mut map = self.adapters.lock().await;
        map.entry(kind.to_string()).or_insert_with(|| Arc::new(ProcessAgent::default())).clone()
    }
}
