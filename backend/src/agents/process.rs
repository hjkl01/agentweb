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
    processes: Arc<Mutex<HashMap<String, Arc<Mutex<Child>>>>>,
    interrupted: Arc<Mutex<HashSet<String>>>,
}

// Tokio tasks can be aborted at any await point. This guard makes cancellation
// clean up the registered process and its interruption marker rather than
// leaving a stale session entry behind.
struct ProcessRunGuard {
    session_id: String,
    execution_id: String,
    process_id: Option<u32>,
    child: Arc<Mutex<Child>>,
    processes: Arc<Mutex<HashMap<String, Arc<Mutex<Child>>>>>,
    interrupted: Arc<Mutex<HashSet<String>>>,
    armed: bool,
}

impl ProcessRunGuard {
    fn new(
        session_id: String,
        execution_id: String,
        process_id: Option<u32>,
        child: Arc<Mutex<Child>>,
        processes: Arc<Mutex<HashMap<String, Arc<Mutex<Child>>>>>,
        interrupted: Arc<Mutex<HashSet<String>>>,
    ) -> Self {
        Self { session_id, execution_id, process_id, child, processes, interrupted, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ProcessRunGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }

        let session_id = self.session_id.clone();
        let execution_id = self.execution_id.clone();
        let process_id = self.process_id;
        let child = self.child.clone();
        let processes = self.processes.clone();
        let interrupted = self.interrupted.clone();

        // Avoid blocking a Tokio worker thread from Drop. Schedule asynchronous
        // termination and remove only this run's entry to protect a later run.
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                #[cfg(unix)]
                if let Some(pid) = process_id {
                    let group = format!("-{pid}");
                    if let Err(error) = Command::new("kill").args(["-KILL", &group]).status().await {
                        tracing::warn!(
                            session_id = %session_id,
                            %error,
                            "failed to kill agent process group after task cancellation"
                        );
                    }
                }

                if let Err(error) = ProcessAdapter::stop_child(&child).await {
                    tracing::warn!(
                        session_id = %session_id,
                        %error,
                        "failed to stop agent child after task cancellation"
                    );
                }

                let mut process_map = processes.lock().await;
                if process_map
                    .get(&execution_id)
                    .is_some_and(|registered| Arc::ptr_eq(registered, &child))
                {
                    process_map.remove(&execution_id);
                    interrupted.lock().await.remove(&execution_id);
                }
            });
        } else {
            // Runtime-shutdown fallback: issue a synchronous process-group kill.
            #[cfg(unix)]
            if let Some(pid) = self.process_id {
                let group = format!("-{pid}");
                let _ = std::process::Command::new("kill").args(["-KILL", &group]).status();
            }
        }
    }
}

impl ProcessAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    fn indicates_missing_native_session(stderr: &str) -> bool {
        let text = stderr.to_ascii_lowercase();
        [
            "session not found",
            "no session found",
            "session does not exist",
            "session doesn't exist",
            "unknown session",
            "invalid session id",
            "could not find session",
            "cannot find session",
            "session file not found",
            "session file does not exist",
            "no such session",
            "thread not found",
            "no thread found",
            "could not resume session",
            "no conversation found",
        ]
        .iter()
        .any(|pattern| text.contains(pattern))
    }
    fn native_session(kind: ProcessKind, value: &Value) -> Option<String> {
        match kind {
            ProcessKind::Codex => codex_events::is_session_event(value)
                .then(|| codex_events::native_session_id(value))
                .flatten(),
            ProcessKind::Claude | ProcessKind::Gemini | ProcessKind::Qwen => {
                let (raw_type, native_id, _, _) = event_parser::parsed(value);
                let typ = raw_type.to_ascii_lowercase();
                if native_id.is_some()
                    && (typ.contains("init") || typ.contains("session") || typ == "system")
                {
                    native_id
                } else {
                    None
                }
            }
            ProcessKind::Pi => {
                let typ = value
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if matches!(
                    typ.as_str(),
                    "session" | "session_start" | "session.started" | "session_starting"
                ) {
                    value
                        .get("id")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .or_else(|| event_parser::parsed(value).1)
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
                    || value.get("role").and_then(Value::as_str) == Some("assistant")
                    || value
                        .get("message")
                        .and_then(|message| message.get("role"))
                        .and_then(Value::as_str)
                        == Some("assistant")
                    || value
                        .get("event")
                        .and_then(|event| event.get("delta"))
                        .and_then(|delta| delta.get("type"))
                        .and_then(Value::as_str)
                        .is_some_and(|kind| kind == "text_delta")
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
        let event_type = value.get("type").and_then(Value::as_str).unwrap_or_default();
        if matches!(kind, ProcessKind::Claude | ProcessKind::Qwen)
            && event_type == "assistant"
        {
            if let Some(final_text) = event_parser::parsed(&value).2 {
                let accumulated = result.lock().await.assistant_text.clone();
                if !accumulated.is_empty() {
                    if accumulated.ends_with(&final_text) {
                        // The final assistant event repeats text already streamed.
                        return;
                    }
                    if let Some(remainder) = final_text.strip_prefix(&accumulated) {
                        // Preserve any suffix that was not included in partial events.
                        if !remainder.is_empty() {
                            let text = remainder.to_owned();
                            result.lock().await.assistant_text.push_str(&text);
                            events
                                .publish(AgentEvent::MessageDelta {
                                    session_id: session_id.into(),
                                    text,
                                })
                                .await;
                        }
                        return;
                    }
                }
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
        self.run_for_execution(kind, config, session_id, session_id, message, events).await
    }

    pub async fn run_for_execution(
        &self,
        kind: ProcessKind,
        config: &AgentConfig,
        session_id: &str,
        execution_id: &str,
        message: &str,
        events: &EventBus,
    ) -> Result<AgentRunResult> {
        // Keep the process-map lock until the newly spawned child is registered.
        // Otherwise two concurrent runs for the same session can both pass the
        // contains_key check and spawn duplicate agent processes.
        let mut processes = self.processes.lock().await;
        if processes.contains_key(execution_id) {
            return Err(anyhow!("session already has a running agent process"));
        }
        let command = commands::build(kind, config, message)?;
        let mut command = Self::wrap_process_group(command);
        command.kill_on_drop(true);
        let mut child_process = command.spawn()?;
        let process_id = child_process.id();
        let stdout = child_process.stdout.take();
        let stderr = child_process.stderr.take();
        let child = Arc::new(Mutex::new(child_process));
        processes.insert(execution_id.to_owned(), child.clone());
        drop(processes);
        let mut run_guard = ProcessRunGuard::new(
            session_id.to_owned(),
            execution_id.to_owned(),
            process_id,
            child.clone(),
            self.processes.clone(),
            self.interrupted.clone(),
        );
        if self.interrupted.lock().await.contains(execution_id) {
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
        let stderr_capture = Arc::new(Mutex::new(String::new()));
        let captured_stderr = stderr_capture.clone();
        let err = tokio::spawn(async move {
            if let Some(stderr) = stderr {
                let mut lines = BufReader::new(stderr).lines();
                while let Some(line) = lines.next_line().await? {
                    if !line.trim().is_empty() {
                        {
                            let mut captured = captured_stderr.lock().await;
                            if captured.len() < 16 * 1024 {
                                captured.push_str(&line);
                                captured.push('\n');
                            }
                        }
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
        // Remove the process entry and its interruption marker atomically with
        // respect to new runs. Otherwise a new run can register between these
        // operations and consume the previous run's stale interruption marker.
        let was_interrupted = {
            let mut processes = self.processes.lock().await;
            let mut interrupted = self.interrupted.lock().await;
            processes.remove(execution_id);
            interrupted.remove(execution_id)
        };
        run_guard.disarm();
        let result = result.lock().await.clone();
        if was_interrupted {
            events
                .publish(AgentEvent::MessageCompleted {
                    session_id: session_id.into(),
                })
                .await;
            return Err(AgentRunError::interrupted(result.native_session_id.clone()).into());
        }
        if let Err(error) = output_result {
            // The session lifecycle layer publishes the user-facing error after
            // it has finalized the session state. Publishing here as well caused
            // duplicate errors and could surface a stale failure after ownership
            // of the session had moved to another worker.
            return Err(AgentRunError::failed(
                error.to_string(),
                result.native_session_id.clone(),
            )
            .into());
        }
        if status.success() {
            events
                .publish(AgentEvent::MessageCompleted {
                    session_id: session_id.into(),
                })
                .await;
            Ok(result)
        } else {
            let stderr = stderr_capture.lock().await.clone();
            let missing_session = Self::indicates_missing_native_session(&stderr);
            let message = if missing_session {
                "The native Agent session could not be found. It may have been deleted; start a new session to continue.".to_owned()
            } else {
                format!("agent exited with status {status}")
            };
            // run_session publishes the error after persisting the final
            // status, so this layer must not emit a duplicate or stale error.
            let error = if missing_session {
                AgentRunError::missing_session(message, result.native_session_id.clone())
            } else {
                AgentRunError::failed(message, result.native_session_id.clone())
            };
            Err(error.into())
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
        self.interrupt_execution(session_id, session_id).await
    }

    pub async fn interrupt_execution(&self, session_id: &str, execution_id: &str) -> Result<()> {
        let child = {
            let processes = self.processes.lock().await;
            let child = processes.get(execution_id).cloned();
            // Record cancellation even before spawn registration. run_for_execution
            // checks this marker immediately after registering the child, closing
            // the stop-vs-start race for this specific execution token.
            self.interrupted.lock().await.insert(execution_id.to_owned());
            child
        };
        if let Some(child) = child {
            tracing::info!(session_id = %session_id, execution_id = %execution_id, "interrupting agent execution");
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
        execution_id: &str,
        message: &str,
        events: &EventBus,
    ) -> Result<AgentRunResult> {
        self.run_for_execution(ProcessKind::Generic, config, session_id, execution_id, message, events)
            .await
    }
    async fn interrupt(&self, session_id: &str, execution_id: &str) -> Result<()> {
        self.interrupt_execution(session_id, execution_id).await
    }
}
pub use super::session::run_session;


#[cfg(test)]
mod tests {
    use super::ProcessAdapter;
    use std::sync::Arc;

    #[test]
    fn detects_explicit_missing_native_session_errors_only() {
        assert!(ProcessAdapter::indicates_missing_native_session(
            "Error: session not found"
        ));
        assert!(ProcessAdapter::indicates_missing_native_session(
            "No session found for the supplied ID"
        ));
        assert!(!ProcessAdapter::indicates_missing_native_session(
            "permission denied while opening session storage"
        ));
        assert!(!ProcessAdapter::indicates_missing_native_session(
            "network timeout"
        ));
    }
    use tokio::{process::Command, sync::Mutex};

    #[cfg(unix)]
    #[tokio::test]
    async fn stop_child_terminates_a_running_process_and_is_safe_to_repeat() {
        let child = Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("sleep should be available on Unix");
        let child = Arc::new(Mutex::new(child));

        ProcessAdapter::stop_child(&child)
            .await
            .expect("the running child should be stopped");
        assert!(
            child.lock().await.try_wait().expect("wait should succeed").is_some(),
            "the child should have exited after stop_child"
        );

        ProcessAdapter::stop_child(&child)
            .await
            .expect("stopping an already exited child should be harmless");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn aborting_run_cleans_up_process_group_and_session_registry() {
        use super::super::adapter::AgentConfig;
        use super::super::commands::ProcessKind;
        use crate::events::EventBus;
        use sqlx::sqlite::SqlitePoolOptions;
        use tokio::time::{sleep, timeout, Duration};

        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory SQLite should be available");
        sqlx::query("CREATE TABLE sessions(id TEXT PRIMARY KEY)")
            .execute(&db)
            .await
            .expect("sessions table should be created");
        sqlx::query("CREATE TABLE agent_events(id INTEGER PRIMARY KEY AUTOINCREMENT,session_id TEXT,worker_id TEXT,event_type TEXT NOT NULL,payload TEXT NOT NULL,created_at TEXT NOT NULL)")
            .execute(&db)
            .await
            .expect("agent_events table should be created");
        let events = EventBus::new(db);
        let adapter = Arc::new(ProcessAdapter::new());
        let config = AgentConfig {
            id: "test".into(),
            command: "sleep".into(),
            working_directory: None,
            native_session_id: None,
            runtime_path: None,
            model: None,
        };

        let run_adapter = adapter.clone();
        let run = tokio::spawn(async move {
            run_adapter
                .run(ProcessKind::Generic, &config, "cancelled-run", "30", &events)
                .await
        });

        let pid = timeout(Duration::from_secs(2), async {
            loop {
                if let Some(child) = adapter.processes.lock().await.get("cancelled-run").cloned() {
                    if let Some(pid) = child.lock().await.id() {
                        break pid;
                    }
                }
                sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the process should register before cancellation");

        run.abort();
        timeout(Duration::from_secs(2), async {
            while !run.is_finished() {
                sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the aborted run should finish promptly");

        timeout(Duration::from_secs(3), async {
            loop {
                let process_registered = adapter.processes.lock().await.contains_key("cancelled-run");
                let interrupted = adapter.interrupted.lock().await.contains("cancelled-run");
                if !process_registered && !interrupted {
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("cancellation cleanup should clear the process and interruption registries");

        let group = format!("-{pid}");
        timeout(Duration::from_secs(2), async {
            loop {
                let probe = Command::new("kill")
                    .args(["-0", &group])
                    .status()
                    .await
                    .expect("kill should be available on Unix");
                if !probe.success() {
                    break;
                }
                sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("the process group should disappear after task cancellation");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn concurrent_runs_for_one_session_do_not_spawn_duplicate_processes() {
        use super::super::adapter::AgentConfig;
        use super::super::commands::ProcessKind;
        use crate::events::EventBus;
        use sqlx::sqlite::SqlitePoolOptions;
        use tokio::time::{sleep, Duration, timeout};

        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory SQLite should be available");
        sqlx::query("CREATE TABLE sessions(id TEXT PRIMARY KEY)")
            .execute(&db)
            .await
            .expect("sessions table should be created");
        sqlx::query("CREATE TABLE agent_events(id INTEGER PRIMARY KEY AUTOINCREMENT,session_id TEXT,worker_id TEXT,event_type TEXT NOT NULL,payload TEXT NOT NULL,created_at TEXT NOT NULL)")
            .execute(&db)
            .await
            .expect("agent_events table should be created");
        let events = EventBus::new(db);
        let adapter = Arc::new(ProcessAdapter::new());
        let config = AgentConfig {
            id: "test".into(),
            command: "sleep".into(),
            working_directory: None,
            native_session_id: None,
            runtime_path: None,
            model: None,
        };

        let first_adapter = adapter.clone();
        let first_config = config.clone();
        let first_events = events.clone();
        let first = tokio::spawn(async move {
            first_adapter
                .run(ProcessKind::Generic, &first_config, "same-session", "30", &first_events)
                .await
        });

        timeout(Duration::from_secs(2), async {
            loop {
                if adapter.processes.lock().await.contains_key("same-session") {
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the first run should register its process");

        let second = adapter
            .run(ProcessKind::Generic, &config, "same-session", "30", &events)
            .await;
        assert!(
            second.is_err(),
            "a concurrent run for the same session must be rejected"
        );
        assert!(
            second
                .expect_err("the duplicate run should fail")
                .to_string()
                .contains("session already has a running agent process")
        );

        adapter
            .interrupt_process("same-session")
            .await
            .expect("the first process should be interruptible");
        let first_result = timeout(Duration::from_secs(3), first)
            .await
            .expect("the first run should finish after interruption")
            .expect("the first task should not panic");
        assert!(first_result.is_err());
        assert!(
            !adapter.processes.lock().await.contains_key("same-session"),
            "the process registry should be cleaned after interruption"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn interrupt_racing_with_natural_exit_cleans_process_and_interrupt_state() {
        use super::super::adapter::AgentConfig;
        use super::super::commands::ProcessKind;
        use crate::events::EventBus;
        use sqlx::sqlite::SqlitePoolOptions;
        use tokio::time::{sleep, timeout, Duration};

        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory SQLite should be available");
        sqlx::query("CREATE TABLE sessions(id TEXT PRIMARY KEY)")
            .execute(&db)
            .await
            .expect("sessions table should be created");
        sqlx::query("CREATE TABLE agent_events(id INTEGER PRIMARY KEY AUTOINCREMENT,session_id TEXT,worker_id TEXT,event_type TEXT NOT NULL,payload TEXT NOT NULL,created_at TEXT NOT NULL)")
            .execute(&db)
            .await
            .expect("agent_events table should be created");
        let events = EventBus::new(db);
        let adapter = Arc::new(ProcessAdapter::new());
        let config = AgentConfig {
            id: "test".into(),
            command: "sleep".into(),
            working_directory: None,
            native_session_id: None,
            runtime_path: None,
            model: None,
        };

        let run_adapter = adapter.clone();
        let run_events = events.clone();
        let run = tokio::spawn(async move {
            run_adapter
                .run(ProcessKind::Generic, &config, "exit-race", "0.1", &run_events)
                .await
        });

        timeout(Duration::from_secs(2), async {
            loop {
                if adapter.processes.lock().await.contains_key("exit-race") {
                    break;
                }
                sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the process should register before the race is triggered");

        let interrupt_adapter = adapter.clone();
        let interrupt = tokio::spawn(async move {
            interrupt_adapter.interrupt_process("exit-race").await
        });

        timeout(Duration::from_secs(3), async {
            let run_result = run.await.expect("the run task should not panic");
            assert!(
                run_result.is_ok()
                    || run_result
                        .as_ref()
                        .expect_err("non-successful result must be an error")
                        .to_string()
                        .contains("interrupted"),
                "the run should either finish naturally or report interruption"
            );
            interrupt
                .await
                .expect("the interrupt task should not panic")
                .expect("interrupting a process that is exiting should be harmless");
        })
        .await
        .expect("run and interrupt should both finish promptly");

        assert!(
            !adapter.processes.lock().await.contains_key("exit-race"),
            "the process registry must be cleared after the race"
        );
        assert!(
            !adapter.interrupted.lock().await.contains("exit-race"),
            "the interruption marker must not leak into the next run"
        );

        // Reusing the same session immediately must not consume a stale interrupt
        // marker left by the previous process lifecycle.
        let next_config = AgentConfig {
            id: "test".into(),
            command: "sleep".into(),
            working_directory: None,
            native_session_id: None,
            runtime_path: None,
            model: None,
        };
        let next_result = timeout(
            Duration::from_secs(3),
            adapter.run(
                ProcessKind::Generic,
                &next_config,
                "exit-race",
                "0.1",
                &events,
            ),
        )
        .await
        .expect("the next run should finish promptly");
        assert!(
            next_result.is_ok(),
            "a subsequent run should not inherit the previous run's interruption"
        );
    }

    // The process monitor must not wait forever when a descendant inherits the
    // agent's output pipes after the direct agent process exits.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn descendant_inherited_pipe_stays_open_until_process_group_is_killed() {
        use tokio::io::AsyncReadExt;
        use tokio::time::{timeout, Duration};

        let mut child = Command::new("setsid")
            .args(["sh", "-c", "sleep 30 & exit 0"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("setsid and sh should be available on Linux");
        let pid = child.id().expect("spawned process should have a PID");
        let mut stdout = child.stdout.take().expect("stdout should be piped");
        let mut stderr = child.stderr.take().expect("stderr should be piped");

        let status = timeout(Duration::from_secs(2), child.wait())
            .await
            .expect("the direct shell should exit promptly")
            .expect("waiting for the shell should succeed");
        assert!(status.success());

        let mut output = Vec::new();
        assert!(
            timeout(Duration::from_millis(100), stdout.read_to_end(&mut output))
                .await
                .is_err(),
            "the background descendant should keep stdout open"
        );

        let group = format!("-{pid}");
        let kill_status = Command::new("kill")
            .args(["-KILL", &group])
            .status()
            .await
            .expect("kill should be available on Linux");
        assert!(kill_status.success(), "the process group should be killed");

        timeout(Duration::from_secs(2), stdout.read_to_end(&mut output))
            .await
            .expect("stdout should close after the descendant is killed")
            .expect("reading stdout should succeed");
        let mut error_output = Vec::new();
        timeout(Duration::from_secs(2), stderr.read_to_end(&mut error_output))
            .await
            .expect("stderr should close after the descendant is killed")
            .expect("reading stderr should succeed");
    }
}
