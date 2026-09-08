use super::{adapter::{AgentAdapter, AgentConfig, AgentRunError, AgentRunResult}, codex_events, commands::{self, ProcessKind}, event_parser, pi_events};
use crate::events::{AgentEvent, EventBus};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::{collections::{HashMap, HashSet}, sync::Arc};
use tokio::{io::{AsyncBufReadExt, BufReader}, process::{Child, Command}, sync::Mutex};

#[derive(Default)]
pub struct ProcessAdapter { processes: Mutex<HashMap<String, Arc<Mutex<Child>>>>, interrupted: Mutex<HashSet<String>> }

impl ProcessAdapter { pub fn new() -> Self { Self::default() }
    fn native_session(kind: ProcessKind, value: &Value) -> Option<String> { match kind { ProcessKind::Codex => codex_events::is_session_event(value).then(|| codex_events::native_session_id(value)).flatten(), ProcessKind::Pi => { let typ = value.get("type").and_then(Value::as_str).unwrap_or_default().to_ascii_lowercase(); if matches!(typ.as_str(), "session_start" | "session.started" | "session_starting") { event_parser::parsed(value).1 } else { None } }, ProcessKind::OpenCode => { let typ = value.get("type").and_then(Value::as_str).unwrap_or_default().to_ascii_lowercase(); if typ == "session.created" || (value.get("sessionID").is_some() && typ.contains("session")) { event_parser::parsed(value).1 } else { None } }, ProcessKind::Generic => None } }
    fn provider_text(kind: ProcessKind, session_id: &str, value: &Value, events: &EventBus) -> Option<String> { match kind { ProcessKind::Pi => pi_events::handle(session_id, value, events), _ => None } }
    fn assistant_event(kind: ProcessKind, value: &Value) -> bool { match kind { ProcessKind::Codex => codex_events::assistant_event(value), ProcessKind::Pi => pi_events::is_assistant(value), _ => { let (typ, _, _, _) = event_parser::parsed(value); let typ = typ.to_ascii_lowercase(); let nested = event_parser::item_type(value); typ.contains("agent_message") || typ.contains("assistant") || typ.contains("text") || typ == "item.completed" || typ == "message_end" || nested.contains("agentmessage") || nested.contains("assistant") } } }
    async fn handle_stdout(kind: ProcessKind, session_id: &str, line: &str, result: &Arc<Mutex<AgentRunResult>>, events: &EventBus) { let Ok(value) = serde_json::from_str::<Value>(line) else { if !line.trim().is_empty() { let text = format!("{line}\n"); result.lock().await.assistant_text.push_str(&text); events.publish(AgentEvent::MessageDelta { session_id: session_id.into(), text }); } return; }; if result.lock().await.native_session_id.is_none() { if let Some(native) = Self::native_session(kind, &value) { result.lock().await.native_session_id = Some(native); events.publish(AgentEvent::SessionStarted { session_id: session_id.into() }); } } if let Some(text) = Self::provider_text(kind, session_id, &value, events) { result.lock().await.assistant_text.push_str(&text); events.publish(AgentEvent::MessageDelta { session_id: session_id.into(), text }); return; } if let Some(text) = event_parser::emit_generic(session_id, &value, line, events) { if Self::assistant_event(kind, &value) { result.lock().await.assistant_text.push_str(&text); events.publish(AgentEvent::MessageDelta { session_id: session_id.into(), text }); } } }

    async fn stop_child(child: &Arc<Mutex<Child>>) -> Result<()> {
        let mut process = child.lock().await;
        #[cfg(unix)]
        if let Some(pid) = process.id() {
            let group = format!("-{pid}");
            let _ = Command::new("kill").args(["-TERM", &group]).status().await;
        }
        let _ = process.kill().await;
        Ok(())
    }

    pub async fn run(&self, kind: ProcessKind, config: &AgentConfig, session_id: &str, message: &str, events: &EventBus) -> Result<AgentRunResult> {
        if self.processes.lock().await.contains_key(session_id) { return Err(anyhow!("session already has a running agent process")); }
        let command = commands::build(kind, config, message)?;
        let mut command = Self::wrap_process_group(command);
        let mut child_process = command.spawn()?;
        let stdout = child_process.stdout.take(); let stderr = child_process.stderr.take();
        let child = Arc::new(Mutex::new(child_process)); self.processes.lock().await.insert(session_id.to_owned(), child.clone());
        if self.interrupted.lock().await.contains(session_id) { Self::stop_child(&child).await?; }
        events.publish(AgentEvent::MessageStarted { session_id: session_id.into() });
        let result = Arc::new(Mutex::new(AgentRunResult { native_session_id: config.native_session_id.clone(), ..Default::default() }));
        let sid = session_id.to_owned(); let out_result = result.clone(); let out_events = events.clone(); let out_sid = sid.clone();
        let out = tokio::spawn(async move { if let Some(stdout) = stdout { let mut lines = BufReader::new(stdout).lines(); while let Some(line) = lines.next_line().await? { Self::handle_stdout(kind, &out_sid, &line, &out_result, &out_events).await; } } Ok::<(), anyhow::Error>(()) });
        let err_sid = sid.clone(); let err_events = events.clone();
        let err = tokio::spawn(async move { if let Some(stderr) = stderr { let mut lines = BufReader::new(stderr).lines(); while let Some(line) = lines.next_line().await? { if !line.trim().is_empty() { err_events.publish(AgentEvent::ToolOutput { session_id: err_sid.clone(), tool: "stderr".into(), output: line }); } } } Ok::<(), anyhow::Error>(()) });
        let status = child.lock().await.wait().await?; out.await??; err.await??;
        self.processes.lock().await.remove(session_id); let was_interrupted = self.interrupted.lock().await.remove(session_id); let result = result.lock().await.clone();
        if was_interrupted { events.publish(AgentEvent::MessageCompleted { session_id: session_id.into() }); return Err(AgentRunError::Interrupted.into()); }
        if status.success() { events.publish(AgentEvent::MessageCompleted { session_id: session_id.into() }); events.publish(AgentEvent::SessionCompleted { session_id: session_id.into() }); Ok(result) } else { events.publish(AgentEvent::Error { session_id: session_id.into(), message: format!("agent exited with status {status}") }); Err(anyhow!("agent exited with status {status}")) }
    }

    #[cfg(unix)]
    fn wrap_process_group(command: Command) -> Command {
        let mut wrapped = Command::new("setsid");
        let std_command = command.as_std();
        if let Some(program) = std_command.get_program().to_str() { wrapped.arg(program); }
        wrapped.args(std_command.get_args());
        wrapped.current_dir(std_command.get_current_dir().unwrap_or_else(|| std::path::Path::new("/")));
        for (key, value) in std_command.get_envs() { if let Some(value) = value { wrapped.env(key, value); } }
        wrapped.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
        wrapped
    }
    #[cfg(not(unix))]
    fn wrap_process_group(command: Command) -> Command { command }

    pub async fn interrupt_process(&self, session_id: &str) -> Result<()> { self.interrupted.lock().await.insert(session_id.to_owned()); let child = self.processes.lock().await.get(session_id).cloned(); if let Some(child) = child { Self::stop_child(&child).await?; } Ok(()) }
}

#[async_trait]
impl AgentAdapter for ProcessAdapter { async fn send_message(&self, config: &AgentConfig, session_id: &str, message: &str, events: &EventBus) -> Result<AgentRunResult> { self.run(ProcessKind::Generic, config, session_id, message, events).await } async fn interrupt(&self, session_id: &str) -> Result<()> { self.interrupt_process(session_id).await } }
pub use super::session::run_session;
