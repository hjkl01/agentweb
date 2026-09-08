use serde_json::Value;

pub fn native_session_id(value: &Value) -> Option<String> {
    value.get("thread_id").and_then(Value::as_str).map(str::to_owned)
}

pub fn is_session_event(value: &Value) -> bool {
    let typ = value.get("type").and_then(Value::as_str).unwrap_or_default().to_ascii_lowercase();
    typ == "thread.started" || typ == "thread_start" || (value.get("thread_id").is_some() && typ.contains("thread"))
}

pub fn assistant_event(value: &Value) -> bool {
    let typ = value.get("type").and_then(Value::as_str).unwrap_or_default().to_ascii_lowercase();
    let item = value.get("item").and_then(|v| v.get("type")).and_then(Value::as_str).unwrap_or_default().to_ascii_lowercase();
    typ.contains("agent_message") || typ.contains("assistant") || typ.contains("text") || typ == "item.completed" || typ == "message_end" || item.contains("agentmessage") || item.contains("assistant")
}
