use crate::events::{AgentEvent, EventBus};
use serde_json::Value;

pub async fn handle(session_id: &str, value: &Value, events: &EventBus) -> Option<String> {
    match value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "message_update" => handle_message_update(session_id, value, events).await,
        "tool_execution_start" => {
            let tool = tool_name(value);
            events
                .publish(AgentEvent::ToolStarted {
                    session_id: session_id.into(),
                    tool: tool.clone(),
                })
                .await;
            if let Some(output) = tool_args(value) {
                publish_tool_output(session_id, tool, output, events).await;
            }
            None
        }
        // `partialResult` streams the tool's output while it is still running.
        "tool_execution_update" => {
            if let Some(output) = result_text(value.get("partialResult")) {
                publish_tool_output(session_id, tool_name(value), output, events).await;
            }
            None
        }
        "tool_execution_end" => {
            let tool = tool_name(value);
            if let Some(output) = result_text(value.get("result")) {
                publish_tool_output(session_id, tool.clone(), output, events).await;
            }
            events
                .publish(AgentEvent::ToolCompleted {
                    session_id: session_id.into(),
                    tool,
                })
                .await;
            None
        }
        _ => None,
    }
}

async fn handle_message_update(
    session_id: &str,
    value: &Value,
    events: &EventBus,
) -> Option<String> {
    let message = value.get("assistantMessageEvent")?;
    match message
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "text_delta" => message
            .get("delta")
            .and_then(Value::as_str)
            .map(str::to_owned),
        "thinking_start" => {
            events
                .publish(AgentEvent::ThinkingStarted {
                    session_id: session_id.into(),
                })
                .await;
            None
        }
        "thinking_delta" => {
            if let Some(text) = message.get("delta").and_then(Value::as_str) {
                events
                    .publish(AgentEvent::ThinkingDelta {
                        session_id: session_id.into(),
                        text: text.into(),
                    })
                    .await;
            }
            None
        }
        "thinking_end" => {
            events
                .publish(AgentEvent::ThinkingCompleted {
                    session_id: session_id.into(),
                })
                .await;
            None
        }
        // `toolcall_*` sub-events only build the call; `tool_execution_*` reports it.
        _ => None,
    }
}

async fn publish_tool_output(session_id: &str, tool: String, output: String, events: &EventBus) {
    events
        .publish(AgentEvent::ToolOutput {
            session_id: session_id.into(),
            tool,
            output,
        })
        .await;
}

fn tool_name(value: &Value) -> String {
    value
        .get("toolName")
        .or_else(|| value.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("tool")
        .to_owned()
}

fn tool_args(value: &Value) -> Option<String> {
    let args = value.get("args")?.as_object()?;
    Some(
        args.iter()
            .map(|(key, value)| match value.as_str() {
                Some(text) => format!("{key}: {text}"),
                None => format!("{key}: {value}"),
            })
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

fn result_text(result: Option<&Value>) -> Option<String> {
    let text = result?
        .get("content")?
        .as_array()?
        .iter()
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<String>();
    (!text.is_empty()).then_some(text)
}

/// Pi reports that the agent loop finished. The process itself can stay alive
/// afterwards when extension child processes (MCP servers) keep it running, so
/// the runner needs this signal to stop it and complete the session.
pub fn is_settled(value: &Value) -> bool {
    value.get("type").and_then(Value::as_str) == Some("agent_settled")
}

pub fn is_assistant(value: &Value) -> bool {
    value.get("type").and_then(Value::as_str) == Some("message_update")
        && value
            .get("assistantMessageEvent")
            .and_then(|v| v.get("type"))
            .and_then(Value::as_str)
            == Some("text_delta")
}
