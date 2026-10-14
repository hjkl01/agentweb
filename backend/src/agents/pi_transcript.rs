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
    if let Some(dir) = std::env::var_os("PI_CODING_AGENT_SESSION_DIR") {
        return Some(PathBuf::from(dir));
    }
    if let Some(dir) = std::env::var_os("PI_CODING_AGENT_DIR") {
        return Some(PathBuf::from(dir).join("sessions"));
    }
    std::env::var_os("HOME").map(|home| {
        PathBuf::from(home)
            .join(".pi")
            .join("agent")
            .join("sessions")
    })
}

/// Read the native transcript from a session-specific workspace state directory.
/// The backend process cannot see environment variables assigned only to the
/// spawned Pi child, so API handlers should pass the persisted workspace path.
pub fn read_messages_from_workspace(
    workspace: &Path,
    native_session_id: &str,
) -> Vec<TranscriptMessage> {
    let Ok(workspace) = workspace.canonicalize() else {
        return Vec::new();
    };
    let state_dir = workspace.join(".agentweb-state");
    let session_dir = state_dir.join("pi-sessions");
    for path in [&state_dir, &session_dir] {
        let Ok(metadata) = std::fs::symlink_metadata(path) else {
            return Vec::new();
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Vec::new();
        }
    }
    read_messages_from_root(&session_dir, native_session_id)
}

fn read_messages_from_root(root: &Path, native_session_id: &str) -> Vec<TranscriptMessage> {
    let suffix = format!("_{native_session_id}.jsonl");
    let Some(path) = find_by_suffix(root, &suffix) else {
        return Vec::new();
    };
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    parse_messages(&content, native_session_id)
}

fn find_by_suffix(dir: &Path, suffix: &str) -> Option<PathBuf> {
    // Bound traversal and never follow symlinks: a workspace-local session
    // directory can be edited by Agent tools and must not redirect this reader
    // into arbitrary host paths or create directory cycles.
    const MAX_ENTRIES: usize = 20_000;
    const MAX_DEPTH: usize = 32;

    let mut pending = vec![(dir.to_path_buf(), 0usize)];
    let mut scanned = 0usize;
    while let Some((current, depth)) = pending.pop() {
        if depth > MAX_DEPTH || scanned >= MAX_ENTRIES {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            scanned += 1;
            if scanned > MAX_ENTRIES {
                return None;
            }
            let path = entry.path();
            let metadata = match std::fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                if depth < MAX_DEPTH {
                    pending.push((path, depth + 1));
                }
            } else if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(suffix))
            {
                return Some(path);
            }
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
    read_messages_from_root(&root, native_session_id)
}

fn parse_messages(content: &str, native_session_id: &str) -> Vec<TranscriptMessage> {
    let entries = content
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            serde_json::from_str::<Value>(line)
                .ok()
                .map(|entry| (index, entry))
        })
        .collect::<Vec<_>>();

    // Pi v2/v3 sessions are trees. On load, Pi restores the leaf from the last
    // persisted entry, then derives the visible branch by following parentId.
    // Older linear sessions have no parentId fields and remain readable as-is.
    let has_tree_links = entries.iter().any(|(_, entry)| entry.get("parentId").is_some());
    let active_ids = if has_tree_links {
        let by_id = entries
            .iter()
            .filter_map(|(_, entry)| {
                entry
                    .get("id")
                    .and_then(Value::as_str)
                    .map(|id| (id.to_owned(), entry))
            })
            .collect::<std::collections::HashMap<_, _>>();
        let mut active = std::collections::HashSet::new();
        let mut current = entries
            .iter()
            .rev()
            .find_map(|(_, entry)| {
                (entry.get("type").and_then(Value::as_str) != Some("session"))
                    .then(|| entry.get("id").and_then(Value::as_str))
                    .flatten()
            });
        while let Some(id) = current {
            if !active.insert(id.to_owned()) {
                break;
            }
            current = by_id
                .get(id)
                .and_then(|entry| entry.get("parentId"))
                .and_then(Value::as_str);
        }
        Some(active)
    } else {
        None
    };

    let mut messages = Vec::new();
    for (index, entry) in entries {
        if let Some(active_ids) = &active_ids {
            let Some(id) = entry.get("id").and_then(Value::as_str) else {
                continue;
            };
            if !active_ids.contains(id) {
                continue;
            }
        }
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
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pi-transcript-{}-{unique}", std::process::id()));
        let nested = dir.join(".agentweb-state").join("pi-sessions").join("--workspaces--");
        std::fs::create_dir_all(&nested).unwrap();
        // Pi uses <timestamp>_<session-id>.jsonl; exercise that exact format.
        let file = nested.join("2026-01-01T00-00-00-000Z_abc123.jsonl");
        std::fs::write(
            &file,
            [
                r#"{"type":"session","id":"abc123"}"#,
                r#"{"type":"message","id":"m1","parentId":null,"timestamp":"2026-01-01T00:00:01Z","message":{"role":"user","content":[{"type":"text","text":"hi"}]}}"#,
                r#"{"type":"message","id":"m2","parentId":"m1","message":{"role":"assistant","content":[{"type":"thinking","thinking":"hmm"},{"type":"text","text":"yo"}]}}"#,
                r#"{"type":"message","id":"m3","parentId":"m1","message":{"role":"user","content":"abandoned branch"}}"#,
                r#"{"type":"message","id":"m4","parentId":"m2","message":{"role":"toolResult","content":[{"type":"text","text":"out"}]}}"#,
            ]
            .join("\n"),
        )
        .unwrap();

        // Exercise the same workspace-scoped path used by the API handler.
        let messages = read_messages_from_workspace(&dir, "abc123");
        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(messages.len(), 2);
        assert!(messages.iter().all(|message| message.content != "abandoned branch"));
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
