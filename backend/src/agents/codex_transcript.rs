//! Read Codex CLI conversation history from its native rollout JSONL files.
//!
//! Codex owns these files under CODEX_HOME/sessions (normally ~/.codex/sessions).
//! Agent Web reads them on demand and never copies message content into SQLite.
use super::pi_transcript::{TranscriptMessage, TranscriptPart};
use serde_json::Value;
use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

const MAX_SCAN_ENTRIES: usize = 50_000;
const MAX_SCAN_DEPTH: usize = 8;
const MAX_TRANSCRIPT_BYTES: u64 = 64 * 1024 * 1024;

fn sessions_root() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| super::config::agent_home().join(".codex"))
        .join("sessions")
}

fn find_rollout(root: &Path, session_id: &str) -> Option<PathBuf> {
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    let mut scanned = 0usize;
    while let Some((directory, depth)) = pending.pop() {
        if depth > MAX_SCAN_DEPTH || scanned >= MAX_SCAN_ENTRIES {
            return None;
        }
        let entries = fs::read_dir(&directory).ok()?;
        for entry in entries {
            scanned += 1;
            if scanned > MAX_SCAN_ENTRIES {
                return None;
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(_) => continue,
            };
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            if file_type.is_dir() {
                pending.push((path, depth + 1));
            } else if file_type.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension == "jsonl")
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().contains(session_id))
            {
                return Some(path);
            }
        }
    }
    None
}

/// Return an empty list when the rollout does not exist or cannot be read.
pub fn read_messages(session_id: &str) -> Vec<TranscriptMessage> {
    if session_id.is_empty() || session_id.contains('/') || session_id.contains('\\') {
        return Vec::new();
    }
    let root = sessions_root();
    let Some(path) = find_rollout(&root, session_id) else {
        return Vec::new();
    };
    let metadata = match fs::metadata(&path) {
        Ok(metadata) if metadata.len() <= MAX_TRANSCRIPT_BYTES => metadata,
        _ => return Vec::new(),
    };
    let _ = metadata;
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return Vec::new(),
    };
    parse_reader(BufReader::new(file))
}

fn parse_reader<R: BufRead>(reader: R) -> Vec<TranscriptMessage> {
    let mut messages = Vec::new();
    for (line_number, line) in reader.lines().enumerate() {
        if line_number >= 1_000_000 {
            break;
        }
        let Ok(line) = line else { break };
        let Ok(record) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(payload) = record.get("payload") else {
            continue;
        };
        let timestamp = record
            .get("timestamp")
            .and_then(Value::as_str)
            .map(str::to_owned);
        match payload.get("type").and_then(Value::as_str) {
            Some("message") => {
                let Some(role) = payload.get("role").and_then(Value::as_str) else {
                    continue;
                };
                if role != "user" && role != "assistant" {
                    continue;
                }
                let mut parts = Vec::new();
                let mut text = String::new();
                if let Some(items) = payload.get("content").and_then(Value::as_array) {
                    for item in items {
                        match item.get("type").and_then(Value::as_str) {
                            Some("input_text" | "output_text" | "text") => {
                                if let Some(value) = item.get("text").and_then(Value::as_str) {
                                    text.push_str(value);
                                    parts.push(TranscriptPart {
                                        kind: "text".into(),
                                        text: Some(value.to_owned()),
                                        ..Default::default()
                                    });
                                }
                            }
                            Some("refusal") => {
                                if let Some(value) = item.get("text").and_then(Value::as_str) {
                                    text.push_str(value);
                                    parts.push(TranscriptPart {
                                        kind: "text".into(),
                                        text: Some(value.to_owned()),
                                        ..Default::default()
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                }
                if text.is_empty() && parts.is_empty() {
                    continue;
                }
                let id = record
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("codex-{line_number}"));
                messages.push(empty_message(id, role, text, timestamp, parts));
            }
            Some("function_call") => {
                let name = payload
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("tool");
                let arguments = payload.get("arguments").cloned();
                let call_id = payload
                    .get("call_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                let part = TranscriptPart {
                    kind: "toolCall".into(),
                    name: Some(name.to_owned()),
                    arguments,
                    tool_call_id: call_id.clone(),
                    ..Default::default()
                };
                let id = record
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("codex-tool-{line_number}"));
                messages.push(empty_message(
                    id,
                    "assistant",
                    String::new(),
                    timestamp,
                    vec![part],
                ));
            }
            Some("function_call_output") => {
                let output = payload
                    .get("output")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let call_id = payload.get("call_id").and_then(Value::as_str).map(str::to_owned);
                let id = record
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("codex-tool-result-{line_number}"));
                let mut message = empty_message(
                    id,
                    "tool",
                    output.clone(),
                    timestamp,
                    vec![TranscriptPart {
                        kind: "text".into(),
                        text: Some(output),
                        ..Default::default()
                    }],
                );
                message.tool_call_id = call_id;
                messages.push(message);
            }
            _ => {}
        }
        }
    messages
}

fn empty_message(
    id: String,
    role: &str,
    content: String,
    created_at: Option<String>,
    parts: Vec<TranscriptPart>,
) -> TranscriptMessage {
    TranscriptMessage {
        id,
        role: role.to_owned(),
        content,
        created_at,
        parts: if parts.is_empty() { None } else { Some(parts) },
        tool_call_id: None,
        is_error: None,
        usage: None,
        tool_name: None,
        duration_ms: None,
        truncated: None,
        diff: None,
        patch: None,
        model: None,
        stop_reason: None,
        error_message: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parses_codex_response_items_without_duplicating_event_messages() {
        let root = std::env::temp_dir().join(format!(
            "agentweb-codex-transcript-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let directory = root.join("2026/11/03");
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("rollout-2026-11-03T10-00-00-test-thread.jsonl");
        fs::write(&path, concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"test-thread\"}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"message\":\"duplicate source\"}}\n",
            "{\"type\":\"response_item\",\"id\":\"u1\",\"timestamp\":\"2026-11-03T10:00:01Z\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"hello\"}]}}\n",
            "{\"type\":\"response_item\",\"id\":\"a1\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"world\"}]}}\n",
            "{\"type\":\"response_item\",\"id\":\"fc1\",\"payload\":{\"type\":\"function_call\",\"name\":\"shell\",\"call_id\":\"call-1\",\"arguments\":\"{}\"}}\n"
        )).unwrap();
        let messages = parse_file(&path);
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].content, "hello");
        assert_eq!(messages[1].content, "world");
        assert_eq!(
            messages[2].parts.as_ref().unwrap()[0].name.as_deref(),
            Some("shell")
        );
        fs::remove_dir_all(root).unwrap();
    }

    fn parse_file(path: &Path) -> Vec<TranscriptMessage> {
        let file = fs::File::open(path).unwrap();
        parse_reader(BufReader::new(file))
    }
}
