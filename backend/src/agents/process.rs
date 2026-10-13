use super::{
    adapter::{AgentAdapter, AgentConfig, AgentRunError, AgentRunResult},
    codex_events,
    commands::{self, ProcessKind},
    event_parser, pi_events,
};
use crate::events::{AgentEvent, EventBus};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::Mutex,
    time::{sleep, Duration},
};

#[derive(Default)]
pub struct ProcessAdapter {
    processes: Mutex<HashMap<String, Arc<Mutex<Child>>>>,
    interrupted: Mutex<HashSet<String>>,
}

impl ProcessAdapter {
    pub fn new() -> Self {
        Self::default()
    }
    fn native_session(kind: ProcessKind, value: &Value) -> Option<String> {
        match kind {
            ProcessKind::Codex => codex_events::is_session_event(value)
                .then(|| codex_events::native_session_id(value))
                .flatten(),
            ProcessKind::Pi => {
                let typ = value
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if matches!(
                    typ.as_str(),
                    "session_start" | "session.started" | "session_starting"
                ) {
                    event_parser::parsed(value).1
                } else {
                    None
                }
            }
            ProcessKind::OpenCode => {
                let typ = value
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if typ == "session.created"
                    || (value.get("sessionID").is_some() && typ.contains("session"))
                {
                    event_parser::parsed(value).1
                } else {
                    None
                }
            }
            ProcessKind::Generic => None,
        }
    }
    async fn provider_text(
        kind: ProcessKind,
        session_id: &str,
        value: &Value,
        events: &EventBus,
    ) -> Option<String> {
        match kind {
            ProcessKind::Pi => pi_events::handle(session_id, value, events).await,
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
                typ.contains("agent_message")
                    || typ.contains("assistant")
                    || typ.contains("text")
                    || typ == "item.completed"
                    || typ == "message_end"
                    || nested.contains("agentmessage")
                    || nested.contains("assistant")
            }
        }
    }
    async fn handle_stdout(
        kind: ProcessKind,
        session_id: &str,
        line: &str,
        result: &Arc<Mutex<AgentRunResult>>,
        events: &EventBus,
    ) {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            if !line.trim().is_empty() {
                let text = format!("{line}\n");
                result.lock().await.assistant_text.push_str(&text);
                events
                    .publish(AgentEvent::MessageDelta {
                        session_id: session_id.into(),
                        text,
                    })
                    .await;
            }
            return;
        };
        if result.lock().await.native_session_id.is_none() {
            if let Some(native) = Self::native_session(kind, &value) {
                result.lock().await.native_session_id = Some(native);
                events
                    .publish(AgentEvent::SessionStarted {
                        session_id: session_id.into(),
                    })
                    .await;
            }
        }
        if let Some(text) = Self::provider_text(kind, session_id, &value, events).await {
            result.lock().await.assistant_text.push_str(&text);
            events
                .publish(AgentEvent::MessageDelta {
                    session_id: session_id.into(),
                    text,
                })
                .await;
            return;
        }
        if let Some(text) = event_parser::emit_generic(session_id, &value, line, events).await {
            if Self::assistant_event(kind, &value) {
                result.lock().await.assistant_text.push_str(&text);
                events
                    .publish(AgentEvent::MessageDelta {
                        session_id: session_id.into(),
                        text,
                    })
                    .await;
            }
        }
    }
    async fn stop_child(child: &Arc<Mutex<Child>>) -> Result<()> {
        let mut process = child.lock().await;
        #[cfg(unix)]
        if let Some(pid) = process.id() {
            let group = format!("-{pid}");
            // Ask the whole process group to exit first; killing the direct child
            // below remains the fallback if the external signal command fails.
            let _ = Command::new("kill").args(["-TERM", &group]).status().await;
        }
        if let Err(error) = process.kill().await {
            // A process that exited after the group signal is already stopped.
            // Only report a failure when the child is still running.
            if process.try_wait()?.is_none() {
                return Err(error.into());
            }
        }
        Ok(())
    }
    pub async fn run(
        &self,
        kind: ProcessKind,
        config: &AgentConfig,
        session_id: &str,
        message: &str,
        events: &EventBus,
    ) -> Result<AgentRunResult> {
        if self.processes.lock().await.contains_key(session_id) {
            return Err(anyhow!("session already has a running agent process"));
        }
        let command = commands::build(kind, config, message)?;
        let mut command = Self::wrap_process_group(command);
        let mut child_process = command.spawn()?;
        let process_id = child_process.id();
        let stdout = child_process.stdout.take();
        let stderr = child_process.stderr.take();
        let child = Arc::new(Mutex::new(child_process));
        self.processes
            .lock()
            .await
            .insert(session_id.to_owned(), child.clone());
        if self.interrupted.lock().await.contains(session_id) {
            // Do not return early here: stdout/stderr readers and the normal
            // process-monitor cleanup still need to run even if the first stop
            // attempt fails. Keeping the child in the process map also allows a
            // subsequent interrupt request to retry stopping it.
            if let Err(error) = Self::stop_child(&child).await {
                tracing::warn!(
                    session_id = %session_id,
                    %error,
                    "failed to stop agent process during startup interruption; continuing cleanup monitoring"
                );
            }
        }
        events
            .publish(AgentEvent::MessageStarted {
                session_id: session_id.into(),
            })
            .await;
        let result = Arc::new(Mutex::new(AgentRunResult {
            native_session_id: config.native_session_id.clone(),
            ..Default::default()
        }));
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
                    if !line.trim().is_empty() {
                        err_events
                            .publish(AgentEvent::ToolOutput {
                                session_id: err_sid.clone(),
                                tool: "stderr".into(),
                                output: line,
                            })
                            .await;
                    }
                }
            }
            Ok::<(), anyhow::Error>(())
        });
        let status = loop {
            match child.lock().await.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {}
                Err(error) => {
                    // A transient wait error must not detach the reader tasks or
                    // discard the process handle. Retry monitoring, and make a
                    // best-effort stop so a still-running child can be reaped.
                    tracing::warn!(
                        session_id = %session_id,
                        %error,
                        "failed to poll agent process status; retrying"
                    );
                    if let Err(stop_error) = Self::stop_child(&child).await {
                        tracing::warn!(
                            session_id = %session_id,
                            %stop_error,
                            "failed to stop agent process after wait error"
                        );
                    }
                }
            }
            sleep(Duration::from_millis(50)).await;
        };
        // Descendants can inherit stdout/stderr and keep the reader tasks alive after the
        // main agent process exits. Bound the drain period so sessions cannot remain stuck.
        let out_abort = out.abort_handle();
        let err_abort = err.abort_handle();
        let output_result = match tokio::time::timeout(Duration::from_secs(5), async {
            let out_result = out.await;
            let err_result = err.await;
            out_result??;
            err_result??;
            Ok::<(), anyhow::Error>(())
        })
        .await
        {
            Ok(result) => result,
            Err(_) => {
                let mut cleanup_error = None;
                #[cfg(unix)]
                if let Some(pid) = process_id {
                    let group = format!("-{pid}");
                    match Command::new("kill").args(["-KILL", &group]).status().await {
                        Ok(status) if status.success() => {}
                        Ok(status) => {
                            cleanup_error = Some(format!(
                                "process-group kill exited with status {status}"
                            ));
                        }
                        Err(error) => {
                            cleanup_error = Some(format!(
                                "failed to execute process-group kill: {error}"
                            ));
                        }
                    }
                    if cleanup_error.is_some() {
                        if let Err(error) = Self::stop_child(&child).await {
                            tracing::warn!(
                                session_id = %session_id,
                                %error,
                                "failed to stop direct child after process-group cleanup failed"
                            );
                        }
                    }
                }
                #[cfg(not(unix))]
                {
                    cleanup_error = Some(
                        "process-group cleanup is unavailable on this platform".to_owned(),
                    );
                }
                if let Some(error) = cleanup_error.as_deref() {
                    tracing::warn!(
                        session_id = %session_id,
                        %error,
                        "failed to clean up descendants after agent output stream timeout"
                    );
                }
                out_abort.abort();
                err_abort.abort();
                Err(anyhow!(
                    "agent output streams did not close within 5 seconds after process exit{}",
                    cleanup_error
                        .map(|error| format!("; descendant cleanup warning: {error}"))
                        .unwrap_or_default()
                ))
            }
        };
        self.processes.lock().await.remove(session_id);
        let was_interrupted = self.interrupted.lock().await.remove(session_id);
        let result = result.lock().await.clone();
        if was_interrupted {
            events
                .publish(AgentEvent::MessageCompleted {
                    session_id: session_id.into(),
                })
                .await;
            return Err(AgentRunError::Interrupted.into());
        }
        if let Err(error) = output_result {
            events
                .publish(AgentEvent::Error {
                    session_id: session_id.into(),
                    message: error.to_string(),
                })
                .await;
            return Err(error);
        }
        if status.success() {
            events
                .publish(AgentEvent::MessageCompleted {
                    session_id: session_id.into(),
                })
                .await;
            Ok(result)
        } else {
            events
                .publish(AgentEvent::Error {
                    session_id: session_id.into(),
                    message: format!("agent exited with status {status}"),
                })
                .await;
            Err(anyhow!("agent exited with status {status}"))
        }
    }
    #[cfg(unix)]
    fn wrap_process_group(command: Command) -> Command {
        let mut wrapped = Command::new("setsid");
        let std_command = command.as_std();
        wrapped.arg(std_command.get_program());
        wrapped.args(std_command.get_args());
        if let Some(dir) = std_command.get_current_dir() {
            wrapped.current_dir(dir);
        }
        for (key, value) in std_command.get_envs() {
            if let Some(value) = value {
                wrapped.env(key, value);
            }
        }
        wrapped
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        wrapped
    }
    #[cfg(not(unix))]
    fn wrap_process_group(command: Command) -> Command {
        command
    }
    pub async fn interrupt_process(&self, session_id: &str) -> Result<()> {
        let child = {
            let processes = self.processes.lock().await;
            let child = processes.get(session_id).cloned();
            if child.is_some() {
                self.interrupted.lock().await.insert(session_id.to_owned());
            }
            child
        };
        if let Some(child) = child {
            Self::stop_child(&child).await?;
        }
        Ok(())
    }
}

#[async_trait]
impl AgentAdapter for ProcessAdapter {
    async fn send_message(
        &self,
        config: &AgentConfig,
        session_id: &str,
        message: &str,
        events: &EventBus,
    ) -> Result<AgentRunResult> {
        self.run(ProcessKind::Generic, config, session_id, message, events)
            .await
    }
    async fn interrupt(&self, session_id: &str) -> Result<()> {
        self.interrupt_process(session_id).await
    }
}
pub use super::session::run_session;
