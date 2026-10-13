//! Read chat messages from a pi native session transcript.
//!
//! pi stores each session as JSONL under `$PI_CODING_AGENT_DIR/sessions` (or
//! `$HOME/.pi/agent/sessions`), one directory per working directory. Every
//! `message` entry carries the provider message that agentweb renders.
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
pub struct TranscriptMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

fn sessions_root() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("PI_CODING_AGENT_DIR") {
        return Some(PathBuf::from(dir).join("sessions"));
    }
    std::env::var("HOME").ok().map(|home| {
        PathBuf::from(home)
            .join(".pi")
            .join("agent")
            .join("sessions")
    })
}

fn find_by_suffix(dir: &Path, suffix: &str) -> Option<PathBuf> {
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_by_suffix(&path, suffix) {
                return Some(found);
            }
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(suffix))
        {
            return Some(path);
        }
    }
    None
}

/// Extract the visible text of a message. pi content is either a plain string
/// or an array of parts; thinking and tool calls belong to activity, not chat.
fn text_content(content: &Value) -> Option<String> {
    match content {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => {
            let text = parts
                .iter()
                .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n");
            (!text.is_empty()).then_some(text)
        }
        _ => None,
    }
}

pub fn read_messages(native_session_id: &str) -> Vec<TranscriptMessage> {
    let Some(root) = sessions_root() else {
        return Vec::new();
    };
    let suffix = format!("_{native_session_id}.jsonl");
    let Some(path) = find_by_suffix(&root, &suffix) else {
        return Vec::new();
    };
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let mut messages = Vec::new();
    for (index, line) in content.lines().enumerate() {
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if entry.get("type").and_then(Value::as_str) != Some("message") {
            continue;
        }
        let Some(message) = entry.get("message") else {
            continue;
        };
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if role != "user" && role != "assistant" {
            continue;
        }
        let Some(text) = message.get("content").and_then(text_content) else {
            continue;
        };
        messages.push(TranscriptMessage {
            id: entry
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("{native_session_id}-{index}")),
            role: role.to_owned(),
            content: text,
            created_at: entry
                .get("timestamp")
                .and_then(Value::as_str)
                .map(str::to_owned),
        });
    }
    messages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_text_and_skips_thinking_and_tool_parts() {
        let content = serde_json::json!([
            {"type": "thinking", "thinking": "secret"},
            {"type": "text", "text": "hello"},
            {"type": "toolCall", "name": "bash", "arguments": {}},
            {"type": "text", "text": "world"},
        ]);
        assert_eq!(text_content(&content).as_deref(), Some("hello\nworld"));
    }

    #[test]
    fn reads_only_user_and_assistant_messages() {
        let dir = std::env::temp_dir().join(format!("pi-transcript-{}", std::process::id()));
        let nested = dir.join("sessions").join("--workspaces--");
        std::fs::create_dir_all(&nested).unwrap();
        let file = nested.join("2026-01-01T00-00-00-000Z_abc123.jsonl");
        std::fs::write(
            &file,
            [
                r#"{"type":"session","id":"abc123"}"#,
                r#"{"type":"message","id":"m1","timestamp":"2026-01-01T00:00:01Z","message":{"role":"user","content":[{"type":"text","text":"hi"}]}}"#,
                r#"{"type":"message","id":"m2","message":{"role":"assistant","content":[{"type":"thinking","thinking":"hmm"},{"type":"text","text":"yo"}]}}"#,
                r#"{"type":"message","id":"m3","message":{"role":"toolResult","content":[{"type":"text","text":"out"}]}}"#,
            ]
            .join("\n"),
        )
        .unwrap();

        std::env::set_var("PI_CODING_AGENT_DIR", &dir);
        let messages = read_messages("abc123");
        std::env::remove_var("PI_CODING_AGENT_DIR");
        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "hi");
        assert_eq!(
            messages[0].created_at.as_deref(),
            Some("2026-01-01T00:00:01Z")
        );
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[1].content, "yo");
    }
}
