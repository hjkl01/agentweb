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
                let mut a = Vec::<String>::new();
                if let Some(thread_id) = &config.native_session_id {
                    a.extend(["exec".into(), "resume".into(), thread_id.clone(), "--json".into(), message.into()]);
                } else {
                    a.extend(["exec".into(), "--json".into(), message.into()]);
                }
                command.args(a);
            }
            "opencode" => {
                let mut a = args;
                a.push("run".into());
                a.push(message.into());
                a.extend(["--format".into(), "json".into()]);
                if let Some(session_id) = &config.native_session_id { a.extend(["--session".into(), session_id.clone()]); }
                command.args(a);
            }
            "pi" => {
                let mut a = args;
                a.extend(["--mode".into(), "json".into()]);
                if let Some(session_id) = &config.native_session_id { a.extend(["--session".into(), session_id.clone()]); }
                a.extend(["-p".into(), message.into()]);
                command.args(a);
            }
            _ => {
                let mut a = args;
                a.push(message.into());
                command.args(a);
            }
        }
        if let Some(dir) = &config.working_directory { command.current_dir(dir); }
        command.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
        Ok(command)
    }

    fn string(v: &Value, keys: &[&str]) -> Option<String> {
        keys.iter().find_map(|key| v.get(*key).and_then(Value::as_str).map(str::to_string))
    }

    fn nested_string(v: &Value, objects: &[&str], keys: &[&str]) -> Option<String> {
        objects.iter().find_map(|object| v.get(*object).and_then(|x| Self::string(x, keys)))
    }

    fn parse_event(v: &Value) -> (String, Option<String>, Option<String>, Option<String>) {
        let typ = Self::string(v, &["type", "event", "method"]).unwrap_or_default();
        let native_session = Self::string(v, &["thread_id", "session_id", "sessionId", "sessionID"])
            .or_else(|| nested_string(v, &["properties", "session", "context"], &["sessionID", "sessionId", "id"]));
        let text = Self::string(v, &["delta", "text", "message", "output", "content"])
            .or_else(|| nested_string(v, &["item", "part", "message", "content"], &["delta", "text", "output"]));
        let name = Self::string(v, &["tool", "tool_name", "toolName", "name", "command"])
            .or_else(|| nested_string(v, &["tool", "item", "part"], &["name", "toolName", "command"]));
        (typ, native_session, text, name)
    }

    fn emit_normalized(session_id: &str, value: &Value, line: &str, events: &EventBus) -> Option<String> {
        let (typ, _native, text, name) = Self::parse_event(value);
        let t = typ.to_ascii_lowercase();
        let is_start = t.contains("start") || t == "turn.started";
        let is_complete = t.contains("complete") || t.contains("finish") || t == "turn.completed";
        let tool_name = name.unwrap_or_else(|| "tool".into());

        if t.contains("tool") {
            if is_start { events.publish(AgentEvent::ToolStarted { session_id: session_id.into(), tool: tool_name.clone() }); }
            if let Some(text) = text.clone() { events.publish(AgentEvent::ToolOutput { session_id: session_id.into(), tool: tool_name.clone(), output: text }); }
            if is_complete { events.publish(AgentEvent::ToolCompleted { session_id: session_id.into(), tool: tool_name }); }
            return None;
        }
        if t.contains("command") || t.contains("shell") || t.contains("exec") {
            if is_start { events.publish(AgentEvent::CommandStarted { session_id: session_id.into(), command: tool_name.clone() }); }
            if let Some(text) = text.clone() { events.publish(AgentEvent::CommandOutput { session_id: session_id.into(), output: text }); }
            if is_complete { events.publish(AgentEvent::CommandCompleted { session_id: session_id.into() }); }
            return None;
        }
        if t.contains("reason") || t.contains("think") || t.contains("thought") {
            if is_start { events.publish(AgentEvent::ThinkingStarted { session_id: session_id.into() }); }
            if let Some(text) = text.clone() { events.publish(AgentEvent::ThinkingDelta { session_id: session_id.into(), text }); }
            if is_complete { events.publish(AgentEvent::ThinkingCompleted { session_id: session_id.into() }); }
            return None;
        }
        if t.contains("file") {
            if let Some(path) = Self::string(value, &["path", "file_path", "filePath"]) {
                let event = if t.contains("creat") { AgentEvent::FileCreated { session_id: session_id.into(), path } }
                    else if t.contains("delet") { AgentEvent::FileDeleted { session_id: session_id.into(), path } }
                    else { AgentEvent::FileModified { session_id: session_id.into(), path } };
                events.publish(event);
            }
            return None;
        }
        if t == "error" || t.contains("failed") || t.contains("failure") {
            let error = Self::string(value, &["message", "error"]).unwrap_or_else(|| line.to_string());
            events.publish(AgentEvent::Error { session_id: session_id.into(), message: error });
        }
        text
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
        let output_events = events.clone();
        let output_result = result.clone();
        let output_task = tokio::spawn(async move {
            if let Some(stdout) = stdout {
                let mut lines = BufReader::new(stdout).lines();
                while let Some(line) = lines.next_line().await? {
                    if let Ok(value) = serde_json::from_str::<Value>(&line) {
                        if let Some(native) = ProcessAgent::parse_event(&value).1 { output_result.lock().await.native_session_id = Some(native); }
                        if let Some(text) = ProcessAgent::emit_normalized(&sid, &value, &line, &output_events) {
                            let typ = ProcessAgent::parse_event(&value).0.to_ascii_lowercase();
                            if typ.contains("agent_message") || typ.contains("assistant") || typ.contains("text") || typ == "item.completed" || typ == "message_update" || typ == "message.part.updated" {
                                output_result.lock().await.assistant_text.push_str(&text);
                                output_events.publish(AgentEvent::MessageDelta { session_id: sid.clone(), text });
                            }
                        }
                    } else if !line.trim().is_empty() {
                        let text = format!("{line}\n");
                        output_result.lock().await.assistant_text.push_str(&text);
                        output_events.publish(AgentEvent::MessageDelta { session_id: sid.clone(), text });
                    }
                }
            }
            Ok::<(), anyhow::Error>(())
        });
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
