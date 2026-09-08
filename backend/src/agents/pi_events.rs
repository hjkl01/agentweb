use crate::events::{AgentEvent, EventBus};
use serde_json::Value;

pub fn handle(session_id: &str, value: &Value, events: &EventBus) -> Option<String> {
    let typ = value.get("type").and_then(Value::as_str).unwrap_or_default();
    if typ != "message_update" {
        return None;
    }
    let message = value.get("assistantMessageEvent")?;
    match message.get("type").and_then(Value::as_str).unwrap_or_default() {
        "text_delta" => message.get("delta").and_then(Value::as_str).map(str::to_owned),
        "thinking_delta" => {
            if let Some(text) = message.get("delta").and_then(Value::as_str) {
                events.publish(AgentEvent::ThinkingDelta { session_id: session_id.into(), text: text.into() });
            }
            None
        }
        _ => None,
    }
}

pub fn is_assistant(value: &Value) -> bool {
    value.get("type").and_then(Value::as_str) == Some("message_update")
        && value.get("assistantMessageEvent").and_then(|v| v.get("type")).and_then(Value::as_str) == Some("text_delta")
}
