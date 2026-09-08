use super::{adapter::{AgentAdapter, AgentConfig, AgentRunResult}, codex_events, event_parser, pi_events};
use crate::events::{AgentEvent, EventBus};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};
use tokio::{io::{AsyncBufReadExt, BufReader}, process::{Child, Command}, sync::Mutex};

#[derive(Clone, Copy)]
pub enum ProcessKind { Codex, OpenCode, Pi, Generic }

#[derive(Default)]
pub struct ProcessAdapter { processes: Mutex<HashMap<String, Arc<Mutex<Child>>>> }

impl ProcessAdapter {
    pub fn new() -> Self { Self::default() }

    fn command_parts(command: &str) -> Result<(String, Vec<String>)> {
        let mut parts = command.split_whitespace();
        let program = parts.next().ok_or_else(|| anyhow!("empty agent command"))?.to_owned();
        Ok((program, parts.map(str::to_owned).collect()))
    }

    pub fn build_command(kind: ProcessKind, config: &AgentConfig, message: &str) -> Result<Command> {
        let (program, base) = Self::command_parts(&config.command)?;
        let mut command = Command::new(program);
        let mut args = base;
        match kind {
            ProcessKind::Codex => {
                args = if let Some(id) = &config.native_session_id {
                    vec!["exec".into(), "resume".into(), id.clone(), "--json".into()]
                } else { vec!["exec".into(), "--json".into()] };
                if let Some(model) = &config.model { args.extend(["--model".into(), model.clone()]); }
                args.push(message.into());
            }
            ProcessKind::OpenCode => {
                args.push("run".into());
                if let Some(model) = &config.model { args.extend(["--model".into(), model.clone()]); }
                if let Some(id) = &config.native_session_id { args.extend(["--session".into(), id.clone()]); }
                args.extend([message.into(), "--format".into(), "json".into()]);
            }
            ProcessKind::Pi => {
                args.extend(["--mode".into(), "json".into()]);
                if let Some(model) = &config.model { args.extend(["--model".into(), model.clone()]); }
                if let Some(id) = &config.native_session_id { args.extend(["--session".into(), id.clone()]); }
                args.extend(["-p".into(), message.into()]);
            }
            ProcessKind::Generic => args.push(message.into()),
        }
        command.args(args);
        if let Some(dir) = &config.working_directory { command.current_dir(dir); }
        if let Some(runtime) = &config.runtime_path {
            command.env("PATH", format!("{}:{}", runtime, std::env::var("PATH").unwrap_or_default()));
        }
        command.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
        Ok(command)
    }

    fn native_session(kind: ProcessKind, value: &Value) -> Option<String> {
        match kind {
            ProcessKind::Codex if codex_events::is_session_event(value) => codex_events::native_session_id(value),
            ProcessKind::Pi => {
                let typ = value.get("type").and_then(Value::as_str).unwrap_or_default().to_ascii_lowercase();
                if typ == "session_start" || typ == "session.started" || typ == "session_starting" {
                    event_parser::parsed(value).1
                } else { None }
            }
            ProcessKind::OpenCode => {
                let typ = value.get("type").and_then(Value::as_str).unwrap_or_default().to_ascii_lowercase();
                if typ == "session.created" || (value.get("sessionID").is_some() && typ.contains("session")) {
                    event_parser::parsed(value).1
                } else { None }
            }
            ProcessKind::Generic => None,
            _ => None,
        }
    }

    fn provider_text(kind: ProcessKind, session_id: &str, value: &Value, events: &EventBus) -> Option<String> {
        match kind {
            ProcessKind::Pi => pi_events::handle(session_id, value, events),
            _ => None,
        }
    }

    fn assistant_event(kind: ProcessKind, value: &Value) -> bool {
        match kind {
            ProcessKind::Codex => codex_events::assistant_event(value),
            ProcessKind::Pi => pi_events::is_assistant(value),
            _ => {
                let (typ, _, _, _) = event_parser::parsed(value);
                let typ = typ.to_ascii_lowercase();
                let nested = event_parser::item_type(value);
                typ.contains("agent_message") || typ.contains("assistant") || typ.contains("text") || typ == "item.completed" || typ == "message_end" || nested.contains("agentmessage") || nested.contains("assistant")
            }
        }
    }

    async fn handle_stdout(kind: ProcessKind, session_id: &str, line: &str, result: &Arc<Mutex<AgentRunResult>>, events: &EventBus) {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            if !line.trim().is_empty() {
                let text = format!("{line}\n");
                result.lock().await.assistant_text.push_str(&text);
                events.publish(AgentEvent::MessageDelta { session_id: session_id.into(), text });
            }
            return;
        };
        if result.lock().await.native_session_id.is_none() {
            if let Some(native) = Self::native_session(kind, &value) {
                result.lock().await.native_session_id = Some(native);
                events.publish(AgentEvent::SessionStarted { session_id: session_id.into() });
            }
        }
        if let Some(text) = Self::provider_text(kind, session_id, &value, events) {
            result.lock().await.assistant_text.push_str(&text);
            events.publish(AgentEvent::MessageDelta { session_id: session_id.into(), text });
            return;
        }
        if let Some(text) = event_parser::emit_generic(session_id, &value, line, events) {
            if Self::assistant_event(kind, &value) {
                result.lock().await.assistant_text.push_str(&text);
                events.publish(AgentEvent::MessageDelta { session_id: session_id.into(), text });
            }
        }
    }

    pub async fn run(&self, kind: ProcessKind, config: &AgentConfig, session_id: &str, message: &str, events: &EventBus) -> Result<AgentRunResult> {
        if self.processes.lock().await.contains_key(session_id) { return Err(anyhow!("session already has a running agent process")); }
        let mut command = Self::build_command(kind, config, message)?;
        let mut child = command.spawn()?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let child = Arc::new(Mutex::new(child));
        self.processes.lock().await.insert(session_id.to_owned(), child.clone());
        events.publish(AgentEvent::MessageStarted { session_id: session_id.into() });
        let result = Arc::new(Mutex::new(AgentRunResult::default()));
        let sid = session_id.to_owned();
        let out_result = result.clone();
        let out_events = events.clone();
        let out_sid = sid.clone();
        let out = tokio::spawn(async move {
            if let Some(stdout) = stdout {
                let mut lines = BufReader::new(stdout).lines();
                while let Some(line) = lines.next_line().await? {
                    Self::handle_stdout(kind, &out_sid, &line, &out_result, &out_events).await;
                }
            }
            Ok::<(), anyhow::Error>(())
        });
        let err_sid = sid.clone();
        let err_events = events.clone();
        let err = tokio::spawn(async move {
            if let Some(stderr) = stderr {
                let mut lines = BufReader::new(stderr).lines();
                while let Some(line) = lines.next_line().await? {
                    if !line.trim().is_empty() { err_events.publish(AgentEvent::ToolOutput { session_id: err_sid.clone(), tool: "stderr".into(), output: line }); }
                }
            }
            Ok::<(), anyhow::Error>(())
        });
        let status = child.lock().await.wait().await?;
        out.await??;
        err.await??;
        self.processes.lock().await.remove(session_id);
        let result = result.lock().await.clone();
        if status.success() {
            events.publish(AgentEvent::MessageCompleted { session_id: session_id.into() });
            events.publish(AgentEvent::SessionCompleted { session_id: session_id.into() });
            Ok(result)
        } else {
            events.publish(AgentEvent::Error { session_id: session_id.into(), message: format!("agent exited with status {status}") });
            Err(anyhow!("agent exited with status {status}"))
        }
    }

    pub async fn interrupt_process(&self, session_id: &str) -> Result<()> {
        if let Some(child) = self.processes.lock().await.remove(session_id) { child.lock().await.kill().await?; }
        Ok(())
    }
}

#[async_trait]
impl AgentAdapter for ProcessAdapter {
    async fn send_message(&self, config: &AgentConfig, session_id: &str, message: &str, events: &EventBus) -> Result<AgentRunResult> {
        self.run(ProcessKind::Generic, config, session_id, message, events).await
    }
    async fn interrupt(&self, session_id: &str) -> Result<()> { self.interrupt_process(session_id).await }
}
