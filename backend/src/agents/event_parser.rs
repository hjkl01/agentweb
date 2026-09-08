use crate::events::{AgentEvent, EventBus};
use serde_json::Value;

pub fn string(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| value.get(*key).and_then(Value::as_str).map(str::to_owned))
}

pub fn nested_string(value: &Value, objects: &[&str], keys: &[&str]) -> Option<String> {
    objects.iter().find_map(|object| value.get(*object).and_then(|v| string(v, keys)))
}

pub fn item_type(value: &Value) -> String {
    nested_string(value, &["item", "message", "assistantMessageEvent"], &["type", "kind"])
        .unwrap_or_default().to_ascii_lowercase()
}

pub fn parsed(value: &Value) -> (String, Option<String>, Option<String>, Option<String>) {
    let typ = string(value, &["type", "event", "method"]).unwrap_or_default();
    let sid = string(value, &["thread_id", "session_id", "sessionId", "sessionID"]).or_else(|| {
        nested_string(value, &["properties", "session", "context"], &["sessionID", "sessionId", "id"])
    });
    let text = string(value, &["delta", "text", "message", "output", "content"])
        .or_else(|| nested_string(value, &["item", "part", "message", "content"], &["delta", "text", "output", "content"]))
        .or_else(|| nested_string(value, &["assistantMessageEvent"], &["delta", "content"]));
    let name = string(value, &["tool", "tool_name", "toolName", "name", "command"])
        .or_else(|| nested_string(value, &["tool", "item", "part"], &["name", "toolName", "command"]));
    (typ, sid, text, name)
}

pub fn text(value: &Value) -> Option<String> {
    if let Some(s) = value.as_str() { return Some(s.to_owned()); }
    if let Some(s) = string(value, &["text", "content", "delta", "output"]) { return Some(s); }
    value.get("content").and_then(Value::as_array).map(|items| items.iter().filter_map(text).collect::<Vec<_>>().join(""))
        .filter(|s| !s.is_empty())
}

pub fn emit_generic(session_id: &str, value: &Value, line: &str, events: &EventBus) -> Option<String> {
    let (typ, _, output, name) = parsed(value);
    let t = typ.to_ascii_lowercase();
    let nested = item_type(value);
    let combined = format!("{t} {nested}");
    let start = t.contains("start") || t == "turn.started" || t == "turn_start";
    let done = t.contains("complete") || t.contains("finish") || t == "turn.completed" || t == "turn_end";
    if combined.contains("commandexecution") || combined.contains("command_execution") {
        let command = string(value, &["command"]).or_else(|| nested_string(value, &["item"], &["command"])).unwrap_or_else(|| "command".into());
        if start { events.publish(AgentEvent::CommandStarted { session_id: session_id.into(), command }); }
        if let Some(output) = string(value, &["aggregated_output"]).or_else(|| nested_string(value, &["item"], &["aggregated_output"])) { events.publish(AgentEvent::CommandOutput { session_id: session_id.into(), output }); }
        if done { events.publish(AgentEvent::CommandCompleted { session_id: session_id.into() }); }
        return None;
    }
    if combined.contains("tool") {
        let tool = name.unwrap_or_else(|| "tool".into());
        if start { events.publish(AgentEvent::ToolStarted { session_id: session_id.into(), tool: tool.clone() }); }
        if let Some(output) = output.clone() { events.publish(AgentEvent::ToolOutput { session_id: session_id.into(), tool: tool.clone(), output }); }
        if done { events.publish(AgentEvent::ToolCompleted { session_id: session_id.into(), tool }); }
        return None;
    }
    if t.contains("reason") || t.contains("think") || t.contains("thought") || nested.contains("reasoning") {
        if start { events.publish(AgentEvent::ThinkingStarted { session_id: session_id.into() }); }
        if let Some(output) = output.clone() { events.publish(AgentEvent::ThinkingDelta { session_id: session_id.into(), text: output }); }
        if done { events.publish(AgentEvent::ThinkingCompleted { session_id: session_id.into() }); }
        return None;
    }
    if t.contains("file") {
        if let Some(path) = string(value, &["path", "file_path", "filePath"]).or_else(|| nested_string(value, &["item"], &["path", "file_path", "filePath"])) {
            events.publish(if t.contains("creat") { AgentEvent::FileCreated { session_id: session_id.into(), path } } else if t.contains("delet") { AgentEvent::FileDeleted { session_id: session_id.into(), path } } else { AgentEvent::FileModified { session_id: session_id.into(), path } });
        }
        return None;
    }
    if t == "error" || t.contains("failed") || t.contains("failure") {
        events.publish(AgentEvent::Error { session_id: session_id.into(), message: string(value, &["message", "error"]).unwrap_or_else(|| line.to_owned()) });
        return None;
    }
    output
}
