use super::adapter::{AgentAdapter, AgentConfig, AgentRunResult};
use crate::events::{AgentEvent, EventBus};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::Mutex,
};

#[derive(Clone, Copy)]
pub enum ProcessKind { Codex, OpenCode, Pi, Generic }

#[derive(Default)]
pub struct ProcessAdapter { processes: Mutex<HashMap<String, Arc<Mutex<Child>>>> }

impl ProcessAdapter {
    pub fn new() -> Self { Self::default() }

    fn command_parts(command: &str) -> Result<(String, Vec<String>)> {
        let mut p = command.split_whitespace();
        let program = p.next().ok_or_else(|| anyhow!("empty agent command"))?.to_string();
        Ok((program, p.map(str::to_owned).collect()))
    }

    pub fn build_command(kind: ProcessKind, config: &AgentConfig, message: &str) -> Result<Command> {
        let (program, base) = Self::command_parts(&config.command)?;
        let mut c = Command::new(program);
        let mut args = base;
        match kind {
            ProcessKind::Codex => {
                args = if let Some(id) = &config.native_session_id {
                    vec!["exec".into(), "resume".into(), id.clone(), "--json".into()]
                } else {
                    vec!["exec".into(), "--json".into()]
                };
                if let Some(model) = &config.model { args.extend(["--model".into(), model.clone()]); }
                args.push(message.into());
            }
            ProcessKind::OpenCode => {
                args.extend(["run".into()]);
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
        c.args(args).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
        if let Some(dir) = &config.working_directory { c.current_dir(dir); }
        Ok(c)
    }

    // ... existing parsing helpers remain unchanged ...
    // The important ownership rule in run(): values used by spawned tasks must
    // be cloned before the first async move closure takes ownership.

    pub async fn run(&self, kind: ProcessKind, config: &AgentConfig, session_id: &str, message: &str, events: &EventBus) -> Result<AgentRunResult> {
        if self.processes.lock().await.contains_key(session_id) { return Err(anyhow!("session already has a running agent process")); }
        let mut cmd = Self::build_command(kind, config, message)?;
        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let child = Arc::new(Mutex::new(child));
        self.processes.lock().await.insert(session_id.to_owned(), child.clone());
        events.publish(AgentEvent::MessageStarted { session_id: session_id.into() });
        let result = Arc::new(Mutex::new(AgentRunResult::default()));
        let sid = session_id.to_owned();
        let ev = events.clone();
        let rr = result.clone();
        let out_sid = sid.clone();
        let out_ev = ev.clone();
        let out = tokio::spawn(async move {
            if let Some(stdout) = stdout {
                let mut lines = BufReader::new(stdout).lines();
                while let Some(line) = lines.next_line().await? {
                    if let Ok(v) = serde_json::from_str::<Value>(&line) {
                        let (typ, native, _, _) = Self::parsed(&v);
                        if rr.lock().await.native_session_id.is_none() && native.is_some() && Self::native_session_event(kind, &v, &typ) {
                            let native = native.unwrap();
                            rr.lock().await.native_session_id = Some(native);
                            out_ev.publish(AgentEvent::SessionStarted { session_id: out_sid.clone() });
                        }
                        if let Some(text) = Self::normalize(&out_sid, &v, &line, &out_ev) {
                            let typ = Self::parsed(&v).0.to_ascii_lowercase();
                            let nested = Self::item_type(&v);
                            let is_assistant = typ.contains("agent_message") || typ.contains("assistant") || typ.contains("text") || typ == "item.completed" || typ == "message_update" || typ == "message_end" || nested.contains("agentmessage") || nested.contains("assistant");
                            if is_assistant {
                                rr.lock().await.assistant_text.push_str(&text);
                                out_ev.publish(AgentEvent::MessageDelta { session_id: out_sid.clone(), text });
                            }
                        }
                    } else if !line.trim().is_empty() {
                        let text = format!("{line}\n");
                        rr.lock().await.assistant_text.push_str(&text);
                        out_ev.publish(AgentEvent::MessageDelta { session_id: out_sid.clone(), text });
                    }
                }
            }
            Ok::<(), anyhow::Error>(())
        });
        let err_sid = sid.clone();
        let err_ev = ev.clone();
        let err = tokio::spawn(async move {
            if let Some(stderr) = stderr {
                let mut lines = BufReader::new(stderr).lines();
                while let Some(line) = lines.next_line().await? {
                    if !line.trim().is_empty() {
                        err_ev.publish(AgentEvent::ToolOutput { session_id: err_sid.clone(), tool: "stderr".into(), output: line });
                    }
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
        } else {
            events.publish(AgentEvent::Error { session_id: session_id.into(), message: format!("agent exited with status {status}") });
        }
        Ok(result)
    }
}
