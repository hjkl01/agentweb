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
pub struct ProcessAgent {
    processes: Mutex<HashMap<String, Arc<Mutex<Child>>>>,
}

impl ProcessAgent {
    fn command_parts(command: &str) -> Result<(String, Vec<String>)> {
        let mut parts = command.split_whitespace();
        let program = parts.next().ok_or_else(|| anyhow!("empty agent command"))?.to_string();
        Ok((program, parts.map(str::to_string).collect()))
    }

    fn build_command(config: &AgentConfig, message: &str) -> Result<Command> {
        let (program, args) = Self::command_parts(&config.command)?;
        let mut command = Command::new(program);

        match config.id.as_str() {
            "codex" => {
                // Codex resume syntax is: codex exec resume <THREAD_ID> --json <PROMPT>.
                // Do not place `resume` after `exec` options or after the prompt.
                let mut a = Vec::<String>::new();
                if let Some(thread_id) = &config.native_session_id {
                    a.extend(["exec".into(), "resume".into(), thread_id.clone(), "--json".into(), message.into()]);
                } else {
                    a.extend(["exec".into(), "--json".into(), message.into()]);
                }
                command.args(a);
            }
            "opencode" => {
                // OpenCode keeps its native session id and resumes it with --session.
                let mut a = args;
                a.push("run".into());
                a.push(message.into());
                a.extend(["--format".into(), "json".into()]);
                if let Some(session_id) = &config.native_session_id {
                    a.extend(["--session".into(), session_id.clone()]);
                }
                command.args(a);
            }
            "pi" => {
                // Pi persists a native session and accepts the session path/id on subsequent runs.
                let mut a = args;
                a.extend(["--mode".into(), "json".into()]);
                if let Some(session_id) = &config.native_session_id {
                    a.extend(["--session".into(), session_id.clone()]);
                }
                a.extend(["-p".into(), message.into()]);
                command.args(a);
            }
            _ => {
                let mut a = args;
                a.push(message.into());
                command.args(a);
            }
        }

        if let Some(dir) = &config.working_directory {
            command.current_dir(dir);
        }
        command.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
        Ok(command)
    }

    fn parse_event(v: &Value) -> (String, Option<String>, Option<String>) {
        let typ = v.get("type").and_then(Value::as_str).unwrap_or("").to_string();
        let native_session = v.get("thread_id").and_then(Value::as_str)
            .or_else(|| v.get("session_id").and_then(Value::as_str))
            .or_else(|| v.get("sessionId").and_then(Value::as_str))
            .or_else(|| v.get("sessionID").and_then(Value::as_str))
            .or_else(|| v.get("properties").and_then(|p| p.get("sessionID")).and_then(Value::as_str))
            .map(str::to_string);
        let text = v.get("delta").and_then(Value::as_str)
            .or_else(|| v.get("text").and_then(Value::as_str))
            .or_else(|| v.get("message").and_then(Value::as_str))
            .or_else(|| v.get("item").and_then(|x| x.get("text")).and_then(Value::as_str))
            .map(str::to_string);
        (typ, native_session, text)
    }
}

#[async_trait]
impl AgentAdapter for ProcessAgent {
    async fn start(&self, _config: &AgentConfig) -> Result<()> { Ok(()) }

    async fn send_message(&self, config: &AgentConfig, session_id: &str, message: &str, events: &EventBus) -> Result<AgentRunResult> {
        if self.processes.lock().await.contains_key(session_id) {
            return Err(anyhow!("session already has a running agent process"));
        }

        let mut command = Self::build_command(config, message)?;
        let mut child = command.spawn()?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let child = Arc::new(Mutex::new(child));
        self.processes.lock().await.insert(session_id.to_string(), child.clone());
        events.publish(AgentEvent::MessageStarted { session_id: session_id.into() });

        let result = Arc::new(Mutex::new(AgentRunResult::default()));
        let sid = session_id.to_string();
        let output_events = events.clone();
        let output_result = result.clone();
        let output_task = tokio::spawn(async move {
            if let Some(stdout) = stdout {
                let mut lines = BufReader::new(stdout).lines();
                while let Some(line) = lines.next_line().await? {
                    if let Ok(value) = serde_json::from_str::<Value>(&line) {
                        let (typ, native_session_id, text) = ProcessAgent::parse_event(&value);
                        if let Some(native) = native_session_id {
                            output_result.lock().await.native_session_id = Some(native);
                        }
                        if let Some(text) = text {
                            if matches!(typ.as_str(),
                                "agent_message_delta" |
                                "agent_message" |
                                "item.completed" |
                                "message_update" |
                                "text_delta" |
                                "assistant" |
                                "message.part.updated") {
                                output_result.lock().await.assistant_text.push_str(&text);
                                output_events.publish(AgentEvent::MessageDelta { session_id: sid.clone(), text });
                            }
                        }
                        if matches!(typ.as_str(), "error" | "turn.failed") {
                            let error = value.get("message").and_then(Value::as_str)
                                .or_else(|| value.get("error").and_then(|x| x.get("message")).and_then(Value::as_str))
                                .unwrap_or(&line);
                            output_events.publish(AgentEvent::Error { session_id: sid.clone(), message: error.to_string() });
                        }
                    } else if !line.trim().is_empty() {
                        output_result.lock().await.assistant_text.push_str(&format!("{line}\n"));
                        output_events.publish(AgentEvent::MessageDelta { session_id: sid.clone(), text: format!("{line}\n") });
                    }
                }
            }
            Ok::<(), anyhow::Error>(())
        });

        // stderr is deliberately not converted into agent.error. Codex and other agents
        // use stderr for diagnostics/logging that are not part of their JSON event protocol.
        let error_task = tokio::spawn(async move {
            if let Some(stderr) = stderr {
                let mut lines = BufReader::new(stderr).lines();
                while lines.next_line().await?.is_some() {}
            }
            Ok::<(), anyhow::Error>(())
        });

        let status = child.lock().await.wait().await?;
        output_task.await??;
        error_task.await??;
        self.processes.lock().await.remove(session_id);

        let result = result.lock().await.clone();
        if status.success() {
            events.publish(AgentEvent::MessageCompleted { session_id: session_id.into() });
            events.publish(AgentEvent::SessionCompleted { session_id: session_id.into() });
        } else {
            events.publish(AgentEvent::Error {
                session_id: session_id.into(),
                message: format!("agent exited with status {status}"),
            });
        }
        Ok(result)
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
