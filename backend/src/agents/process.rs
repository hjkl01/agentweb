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
        c.args(args);
        if let Some(dir) = &config.working_directory { c.current_dir(dir); }
        if let Some(runtime) = &config.runtime_path {
            c.env("PATH", format!("{}:{}", runtime, std::env::var("PATH").unwrap_or_default()));
        }
        c.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
        Ok(c)
    }

    fn string(v: &Value, keys: &[&str]) -> Option<String> {
        keys.iter().find_map(|k| v.get(*k).and_then(Value::as_str).map(str::to_owned))
    }

    fn nested_string(v: &Value, objects: &[&str], keys: &[&str]) -> Option<String> {
        objects.iter().find_map(|o| v.get(*o).and_then(|x| Self::string(x, keys)))
    }

    fn item_type(v: &Value) -> String {
        Self::nested_string(v, &["item", "message", "assistantMessageEvent"], &["type", "kind"])
            .unwrap_or_default()
            .to_ascii_lowercase()
    }

    fn parsed(v: &Value) -> (String, Option<String>, Option<String>, Option<String>) {
        let typ = Self::string(v, &["type", "event", "method"]).unwrap_or_default();
        let sid = Self::string(v, &["thread_id", "session_id", "sessionId", "sessionID"]).or_else(|| {
            Self::nested_string(v, &["properties", "session", "context"], &["sessionID", "sessionId", "id"])
        });
        let text = Self::string(v, &["delta", "text", "message", "output", "content"])
            .or_else(|| Self::nested_string(v, &["item", "part", "message", "content"], &["delta", "text", "output", "content"]))
            .or_else(|| Self::nested_string(v, &["assistantMessageEvent"], &["delta", "content"]));
        let name = Self::string(v, &["tool", "tool_name", "toolName", "name", "command"])
            .or_else(|| Self::nested_string(v, &["tool", "item", "part"], &["name", "toolName", "command"]))
            .or_else(|| Self::string(v, &["toolName"]));
        (typ, sid, text, name)
    }

    fn native_session_event(kind: ProcessKind, value: &Value, typ: &str) -> bool {
        let t = typ.to_ascii_lowercase();
        match kind {
            ProcessKind::Codex => t == "thread.started" || t == "thread_start" || (value.get("thread_id").is_some() && t.contains("thread")),
            ProcessKind::Pi => t == "session_start" || t == "session.started" || t == "session_starting" || (value.get("session_id").is_some() && t.contains("session")),
            ProcessKind::OpenCode => t == "session.created" || (value.get("sessionID").is_some() && t.contains("session")),
            ProcessKind::Generic => false,
        }
    }

    fn text_from_json(v: &Value) -> Option<String> {
        if let Some(s) = v.as_str() { return Some(s.to_owned()); }
        if let Some(s) = Self::string(v, &["text", "content", "delta", "output"]) { return Some(s); }
        if let Some(content) = v.get("content").and_then(Value::as_array) {
            let text = content.iter().filter_map(Self::text_from_json).collect::<Vec<_>>().join("");
            if !text.is_empty() { return Some(text); }
        }
        None
    }

    fn normalize(session_id: &str, v: &Value, line: &str, events: &EventBus) -> Option<String> {
        let (typ, _, text, name) = Self::parsed(v);
        let t = typ.to_ascii_lowercase();
        let nested = Self::item_type(v);
        let combined = format!("{t} {nested}");
        let start = t.contains("start") || t == "turn.started" || t == "turn_start";
        let done = t.contains("complete") || t.contains("finish") || t == "turn.completed" || t == "turn_end";

        if t == "message_update" {
            if let Some(kind) = v.get("assistantMessageEvent").and_then(|x| x.get("type")).and_then(Value::as_str) {
                match kind {
                    "text_delta" => return v.get("assistantMessageEvent").and_then(|x| x.get("delta")).and_then(Value::as_str).map(str::to_owned),
                    "thinking_delta" => {
                        if let Some(x) = v.get("assistantMessageEvent").and_then(|x| x.get("delta")).and_then(Value::as_str) {
                            events.publish(AgentEvent::ThinkingDelta { session_id: session_id.into(), text: x.to_owned() });
                        }
                        return None;
                    }
                    _ => {}
                }
            }
        }

        if t == "tool_execution_start" {
            let tool = Self::string(v, &["toolName", "tool_name", "name"]).unwrap_or_else(|| "tool".into());
            events.publish(AgentEvent::ToolStarted { session_id: session_id.into(), tool });
            return None;
        }
        if t == "tool_execution_update" {
            let tool = Self::string(v, &["toolName", "tool_name", "name"]).unwrap_or_else(|| "tool".into());
            if let Some(x) = v.get("partialResult").and_then(Self::text_from_json) {
                events.publish(AgentEvent::ToolOutput { session_id: session_id.into(), tool, output: x });
            }
            return None;
        }
        if t == "tool_execution_end" {
            let tool = Self::string(v, &["toolName", "tool_name", "name"]).unwrap_or_else(|| "tool".into());
            if let Some(x) = v.get("result").and_then(Self::text_from_json) {
                events.publish(AgentEvent::ToolOutput { session_id: session_id.into(), tool: tool.clone(), output: x });
            }
            events.publish(AgentEvent::ToolCompleted { session_id: session_id.into(), tool });
            return None;
        }

        if combined.contains("commandexecution") || combined.contains("command_execution") {
            let command = Self::string(v, &["command"]).or_else(|| Self::nested_string(v, &["item"], &["command"])).unwrap_or_else(|| "command".into());
            if start { events.publish(AgentEvent::CommandStarted { session_id: session_id.into(), command }); }
            if let Some(x) = v.get("aggregated_output").and_then(Value::as_str).or_else(|| v.get("item").and_then(|i| i.get("aggregated_output")).and_then(Value::as_str)) {
                events.publish(AgentEvent::CommandOutput { session_id: session_id.into(), output: x.to_owned() });
            }
            if done { events.publish(AgentEvent::CommandCompleted { session_id: session_id.into() }); }
            return None;
        }
        if combined.contains("mcp_tool_call") || combined.contains("toolcall") || combined.contains("tool_call") || t.contains("tool") {
            let tool = name.unwrap_or_else(|| "tool".into());
            if start { events.publish(AgentEvent::ToolStarted { session_id: session_id.into(), tool: tool.clone() }); }
            if let Some(x) = text.clone() { events.publish(AgentEvent::ToolOutput { session_id: session_id.into(), tool: tool.clone(), output: x }); }
            if done { events.publish(AgentEvent::ToolCompleted { session_id: session_id.into(), tool }); }
            return None;
        }
        if t.contains("reason") || t.contains("think") || t.contains("thought") || nested.contains("reasoning") {
            if start { events.publish(AgentEvent::ThinkingStarted { session_id: session_id.into() }); }
            if let Some(x) = text.clone() { events.publish(AgentEvent::ThinkingDelta { session_id: session_id.into(), text: x }); }
            if done { events.publish(AgentEvent::ThinkingCompleted { session_id: session_id.into() }); }
            return None;
        }
        if t.contains("file") {
            if let Some(path) = Self::string(v, &["path", "file_path", "filePath"]).or_else(|| Self::nested_string(v, &["item"], &["path", "file_path", "filePath"])) {
                events.publish(if t.contains("creat") { AgentEvent::FileCreated { session_id: session_id.into(), path } } else if t.contains("delet") { AgentEvent::FileDeleted { session_id: session_id.into(), path } } else { AgentEvent::FileModified { session_id: session_id.into(), path } });
            }
            return None;
        }
        if t == "error" || t.contains("failed") || t.contains("failure") {
            events.publish(AgentEvent::Error { session_id: session_id.into(), message: Self::string(v, &["message", "error"]).unwrap_or_else(|| line.to_owned()) });
            return None;
        }
        text
    }

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
