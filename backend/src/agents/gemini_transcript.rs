//! Read Gemini CLI's native project-scoped chat recording files.
//!
//! Gemini stores append-only JSONL under ~/.gemini/tmp/<project-id>/chats.
//! Agent Web reads these files on demand and does not copy chat content to SQLite.
use super::pi_transcript::{TranscriptMessage, TranscriptPart};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

const MAX_PROJECT_ENTRIES: usize = 50_000;
const MAX_TRANSCRIPT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TRANSCRIPT_LINES: usize = 1_000_000;
const MAX_LINE_BYTES: usize = 2 * 1024 * 1024;
const MAX_SCAN_DEPTH: usize = 3;

fn gemini_home() -> PathBuf {
    std::env::var_os("GEMINI_CLI_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(super::config::agent_home)
}

fn temp_roots() -> [PathBuf; 2] {
    let home = gemini_home();
    [
        home.join(".gemini").join("tmp"),
        home.join(".cache").join(".gemini").join("tmp"),
    ]
}

fn safe_directory(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_dir() && !metadata.file_type().is_symlink())
        .unwrap_or(false)
}

fn find_session_file(root: &Path, session_id: &str, project_hash: &str) -> Option<PathBuf> {
    if !safe_directory(root) {
        return None;
    }
    let short_id = session_id.get(..8)?;
    let projects = fs::read_dir(root).ok()?;
    let mut scanned = 0usize;
    for project in projects {
        scanned += 1;
        if scanned > MAX_PROJECT_ENTRIES {
            return None;
        }
        let Ok(project) = project else { continue };
        let Ok(project_kind) = project.file_type() else {
            continue;
        };
        if !project_kind.is_dir() || project_kind.is_symlink() {
            continue;
        }
        let chats = project.path().join("chats");
        if !safe_directory(&chats) {
            continue;
        }
        let mut pending = vec![(chats, 0usize)];
        while let Some((directory, depth)) = pending.pop() {
            if !safe_directory(&directory) {
                continue;
            }
            let entries = fs::read_dir(&directory).ok()?;
            for entry in entries {
                scanned += 1;
                if scanned > MAX_PROJECT_ENTRIES {
                    return None;
                }
                let Ok(entry) = entry else { continue };
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                let path = entry.path();
                if kind.is_symlink() {
                    continue;
                }
                if kind.is_dir() {
                    if depth < MAX_SCAN_DEPTH {
                        pending.push((path, depth + 1));
                    }
                    continue;
                }
                if !kind.is_file() {
                    continue;
                }
                let name = entry.file_name();
                let name = name.to_string_lossy();
                let candidate = (name.ends_with(".jsonl") || name.ends_with(".json"))
                    && (name == format!("{session_id}.jsonl")
                        || name == format!("{session_id}.json")
                        || (name.starts_with("session-")
                            && name.contains(short_id)
                            && (name.ends_with(".jsonl") || name.ends_with(".json"))));
                if !candidate {
                    continue;
                }
                let metadata = fs::metadata(&path).ok()?;
                if metadata.len() > MAX_TRANSCRIPT_BYTES {
                    continue;
                }
                if file_has_session_id(&path, session_id, project_hash) {
                    return Some(path);
                }
            }
        }
    }
    None
}

fn file_has_session_id(path: &Path, session_id: &str, project_hash: &str) -> bool {
    let Ok(file) = fs::File::open(path) else {
        return false;
    };
    let mut reader = BufReader::new(file);
    let mut first_line = String::new();
    if reader.read_line(&mut first_line).is_err() {
        return false;
    }
    serde_json::from_str::<Value>(&first_line)
        .ok()
        .is_some_and(|value| {
            value.get("sessionId").and_then(Value::as_str) == Some(session_id)
                && value.get("projectHash").and_then(Value::as_str) == Some(project_hash)
        })
}

/// Return an empty list when the native session cannot be located or read.
pub fn read_messages(session_id: &str, workspace: &str) -> Vec<TranscriptMessage> {
    if session_id.is_empty()
        || session_id.len() > 128
        || !session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Vec::new();
    }
    let workspace = Path::new(workspace);
    if !workspace.is_absolute() {
        return Vec::new();
    }
    let project_hash = format!(
        "{:x}",
        Sha256::digest(workspace.to_string_lossy().as_bytes())
    );
    let Some(path) = temp_roots()
        .into_iter()
        .find_map(|root| find_session_file(&root, session_id, &project_hash))
    else {
        return Vec::new();
    };
    let Ok(metadata) = fs::metadata(&path) else {
        return Vec::new();
    };
    if metadata.len() > MAX_TRANSCRIPT_BYTES {
        return Vec::new();
    }
    let Ok(file) = fs::File::open(path) else {
        return Vec::new();
    };
    parse_reader(BufReader::new(file), session_id)
}

fn text_from_content(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

fn content_parts(content: &Value) -> Vec<TranscriptPart> {
    match content {
        Value::String(text) if !text.is_empty() => vec![TranscriptPart {
            kind: "text".into(),
            text: Some(text.clone()),
            ..Default::default()
        }],
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                let text = item.get("text").and_then(Value::as_str)?;
                Some(TranscriptPart {
                    kind: "text".into(),
                    text: Some(text.to_owned()),
                    ..Default::default()
                })
            })
            .collect(),
        _ => Vec::new(),
    }
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
        parts: (!parts.is_empty()).then_some(parts),
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

fn tool_result_text(result: &Value) -> String {
    match result {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(_) => result
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        _ => String::new(),
    }
}

fn parse_message(
    record: &Value,
    messages: &mut Vec<TranscriptMessage>,
    positions: &mut HashMap<String, usize>,
) {
    let Some(id) = record.get("id").and_then(Value::as_str) else {
        return;
    };
    let Some(kind) = record.get("type").and_then(Value::as_str) else {
        return;
    };
    let timestamp = record
        .get("timestamp")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let content = record.get("content").unwrap_or(&Value::Null);

    let mut message = match kind {
        "user" => {
            let text = text_from_content(content);
            if text.is_empty() {
                return;
            }
            empty_message(
                id.to_owned(),
                "user",
                text,
                timestamp,
                content_parts(content),
            )
        }
        "gemini" => {
            let text = text_from_content(content);
            let mut parts = content_parts(content);
            let tool_calls = record.get("toolCalls").and_then(Value::as_array);
            if let Some(calls) = tool_calls {
                for call in calls {
                    let Some(name) = call.get("name").and_then(Value::as_str) else {
                        continue;
                    };
                    parts.push(TranscriptPart {
                        kind: "toolCall".into(),
                        name: Some(name.to_owned()),
                        arguments: call.get("args").cloned(),
                        tool_call_id: call.get("id").and_then(Value::as_str).map(str::to_owned),
                        ..Default::default()
                    });
                }
            }
            if text.is_empty() && parts.is_empty() {
                return;
            }
            let mut message = empty_message(id.to_owned(), "assistant", text, timestamp, parts);
            message.model = record
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_owned);
            message.usage = record.get("tokens").cloned();
            message
        }
        _ => return,
    };

    let message_created_at = message.created_at.clone();
    if let Some(index) = positions.get(id).copied() {
        messages[index] = message;
    } else {
        positions.insert(id.to_owned(), messages.len());
        messages.push(message);
    }

    if kind == "gemini" {
        if let Some(calls) = record.get("toolCalls").and_then(Value::as_array) {
            for call in calls {
                let Some(call_id) = call.get("id").and_then(Value::as_str) else {
                    continue;
                };
                let Some(result) = call.get("result").filter(|result| !result.is_null()) else {
                    continue;
                };
                let text = tool_result_text(result);
                let tool_message_id = format!("{id}-tool-{call_id}");
                let mut tool_message = empty_message(
                    tool_message_id.clone(),
                    "tool",
                    text.clone(),
                    call.get("timestamp")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .or_else(|| message_created_at.clone()),
                    if text.is_empty() {
                        Vec::new()
                    } else {
                        vec![TranscriptPart {
                            kind: "text".into(),
                            text: Some(text),
                            ..Default::default()
                        }]
                    },
                );
                tool_message.tool_call_id = Some(call_id.to_owned());
                tool_message.tool_name =
                    call.get("name").and_then(Value::as_str).map(str::to_owned);
                tool_message.is_error = call
                    .get("status")
                    .and_then(Value::as_str)
                    .map(|status| status == "error");
                if let Some(index) = positions.get(&tool_message_id).copied() {
                    messages[index] = tool_message;
                } else {
                    positions.insert(tool_message_id, messages.len());
                    messages.push(tool_message);
                }
            }
        }
    }
}

fn parse_reader<R: BufRead>(mut reader: R, expected_session_id: &str) -> Vec<TranscriptMessage> {
    let mut messages = Vec::new();
    let mut positions = HashMap::new();
    let mut line = Vec::new();

    for line_number in 0..MAX_TRANSCRIPT_LINES {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) => break,
            Ok(_) if line.len() > MAX_LINE_BYTES => continue,
            Ok(_) => {}
            Err(_) => break,
        }
        let Ok(record) = serde_json::from_slice::<Value>(&line) else {
            continue;
        };
        if line_number == 0 {
            if record.get("sessionId").and_then(Value::as_str) != Some(expected_session_id) {
                return Vec::new();
            }
            if let Some(legacy_messages) = record.get("messages").and_then(Value::as_array) {
                for item in legacy_messages {
                    parse_message(item, &mut messages, &mut positions);
                }
            }
            continue;
        }
        if let Some(rewind_id) = record.get("$rewindTo").and_then(Value::as_str) {
            if let Some(index) = positions.get(rewind_id).copied() {
                messages.truncate(index);
                positions.retain(|_, position| *position < index);
            } else {
                messages.clear();
                positions.clear();
            }
            continue;
        }
        if let Some(patch) = record.get("$patch") {
            if let Some(id) = patch.get("id").and_then(Value::as_str) {
                if let Some(index) = positions.get(id).copied() {
                    if let Some(content) = patch.get("content") {
                        let text = text_from_content(content);
                        messages[index].content = text;
                        messages[index].parts = Some(content_parts(content));
                    }
                }
            }
            continue;
        }
        if let Some(set) = record.get("$set") {
            if let Some(legacy_messages) = set.get("messages").and_then(Value::as_array) {
                messages.clear();
                positions.clear();
                for item in legacy_messages {
                    parse_message(item, &mut messages, &mut positions);
                }
            }
            continue;
        }
        parse_message(&record, &mut messages, &mut positions);
    }
    messages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gemini_messages_tools_usage_and_updates() {
        let fixture = [
            r#"{"sessionId":"session-123","projectHash":"hash","startTime":"2026-01-01T00:00:00Z"}"#,
            r#"{"id":"u1","timestamp":"2026-01-01T00:00:01Z","type":"user","content":[{"text":"hello"}]}"#,
            r#"{"id":"a1","timestamp":"2026-01-01T00:00:02Z","type":"gemini","content":[{"text":"working"}],"model":"gemini-2.5-pro","tokens":{"input":10,"output":5},"toolCalls":[{"id":"call-1","name":"read_file","args":{"path":"a.txt"},"result":[{"text":"file data"}],"status":"success"}]}"#,
            r#"{"$patch":{"id":"a1","content":[{"text":"done"}]}}"#,
        ]
        .join("\n");
        let messages = parse_reader(BufReader::new(fixture.as_bytes()), "session-123");
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[1].content, "done");
        assert_eq!(messages[1].model.as_deref(), Some("gemini-2.5-pro"));
        assert_eq!(
            messages[1].usage.as_ref().and_then(|v| v["input"].as_i64()),
            Some(10)
        );
        assert_eq!(messages[1].parts.as_ref().unwrap()[0].kind, "text");
        assert_eq!(messages[2].role, "tool");
        assert_eq!(messages[2].content, "file data");
        assert_eq!(messages[2].tool_call_id.as_deref(), Some("call-1"));
    }

    #[test]
    fn rewinds_messages_and_rejects_mismatched_session() {
        let fixture = [
            r#"{"sessionId":"session-123","projectHash":"hash"}"#,
            r#"{"id":"u1","type":"user","content":"first"}"#,
            r#"{"id":"a1","type":"gemini","content":"second"}"#,
            r#"{"$rewindTo":"a1"}"#,
        ]
        .join("\n");
        let messages = parse_reader(BufReader::new(fixture.as_bytes()), "session-123");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "first");
        assert!(parse_reader(BufReader::new(fixture.as_bytes()), "other").is_empty());
    }

    #[test]
    fn finds_session_by_native_id_and_project_hash() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("agentweb-gemini-{unique}"));
        let chats = root.join("project-slug").join("chats");
        fs::create_dir_all(&chats).unwrap();
        let session_id = "12345678-abcd";
        let project_hash = "project-hash";
        let file = chats.join("session-2026-11-03-12345678.jsonl");
        fs::write(
            &file,
            format!(
                "{{\"sessionId\":\"{session_id}\",\"projectHash\":\"{project_hash}\"}}\\n"
            ),
        )
        .unwrap();

        assert_eq!(
            find_session_file(&root, session_id, project_hash),
            Some(file)
        );
        assert!(find_session_file(&root, session_id, "other-project").is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reads_legacy_json_with_embedded_messages() {
        let fixture = r#"{"sessionId":"session-123","projectHash":"hash","messages":[{"id":"u1","type":"user","content":"legacy prompt"},{"id":"a1","type":"gemini","content":"legacy answer"}]}"#;
        let messages = parse_reader(BufReader::new(fixture.as_bytes()), "session-123");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content, "legacy prompt");
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[1].content, "legacy answer");
    }

    #[test]
    fn rejects_unsafe_session_ids() {
        assert!(read_messages("../escape", "/tmp/project").is_empty());
        assert!(read_messages("session/escape", "/tmp/project").is_empty());
    }
}
