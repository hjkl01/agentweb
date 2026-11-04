//! Read Claude Code's native project-scoped JSONL session transcripts.
//!
//! Claude owns these files under CLAUDE_CONFIG_DIR/projects (normally
//! ~/.claude/projects). Agent Web reads them on demand and never stores message
//! content in SQLite.
use super::pi_transcript::{TranscriptMessage, TranscriptPart};
use serde_json::Value;
use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

const MAX_PROJECT_ENTRIES: usize = 50_000;
const MAX_TRANSCRIPT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TRANSCRIPT_LINES: usize = 1_000_000;

fn projects_root() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| super::config::agent_home().join(".claude"))
        .join("projects")
}

fn find_transcript(root: &Path, session_id: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(root).ok()?;
    let mut scanned = 0usize;
    for entry in entries {
        scanned += 1;
        if scanned > MAX_PROJECT_ENTRIES {
            return None;
        }
        let Ok(entry) = entry else {
            continue;
        };
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if !kind.is_dir() || kind.is_symlink() {
            continue;
        }
        let path = entry.path().join(format!("{session_id}.jsonl"));
        if path.is_file() && !fs::symlink_metadata(&path).ok()?.file_type().is_symlink() {
            return Some(path);
        }
    }
    None
}

/// Return an empty list when the session transcript cannot be located or read.
pub fn read_messages(session_id: &str) -> Vec<TranscriptMessage> {
    if session_id.is_empty()
        || !session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Vec::new();
    }
    let Some(path) = find_transcript(&projects_root(), session_id) else {
        return Vec::new();
    };
    match fs::metadata(&path) {
        Ok(metadata) if metadata.len() <= MAX_TRANSCRIPT_BYTES => {}
        _ => return Vec::new(),
    }
    let Ok(file) = fs::File::open(path) else {
        return Vec::new();
    };
    parse_reader(BufReader::new(file))
}

fn parse_reader<R: BufRead>(reader: R) -> Vec<TranscriptMessage> {
    let mut messages = Vec::new();
    for (line_number, line) in reader.lines().enumerate() {
        if line_number >= MAX_TRANSCRIPT_LINES {
            break;
        }
        let Ok(line) = line else {
            break;
        };
        let Ok(record) = serde_json::from_str::<Value>(&line) else {
            // A live Claude session can have a partially-written final line.
            continue;
        };
        let Some(role) = record.get("type").and_then(Value::as_str) else {
            continue;
        };
        if role != "user" && role != "assistant" {
            continue;
        }
        if record
            .get("isSidechain")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            continue;
        }
        let Some(message) = record.get("message") else {
            continue;
        };
        let id = record
            .get("uuid")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| message.get("id").and_then(Value::as_str).map(str::to_owned))
            .unwrap_or_else(|| format!("claude-{line_number}"));
        let created_at = record
            .get("timestamp")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let model = message
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let usage = message.get("usage").cloned();
        let Some(content) = message.get("content") else {
            continue;
        };
        let blocks = match content {
            Value::String(text) => vec![Value::String(text.clone())],
            Value::Array(blocks) => blocks.clone(),
            _ => continue,
        };

        let mut text = String::new();
        let mut parts = Vec::new();
        for (index, block) in blocks.iter().enumerate() {
            if let Some(value) = block.as_str() {
                append_text(&mut text, &mut parts, value);
                continue;
            }
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(value) = block.get("text").and_then(Value::as_str) {
                        append_text(&mut text, &mut parts, value);
                    }
                }
                Some("thinking") => {
                    if let Some(value) = block.get("thinking").and_then(Value::as_str) {
                        parts.push(TranscriptPart {
                            kind: "thinking".into(),
                            text: Some(value.to_owned()),
                            ..Default::default()
                        });
                    }
                }
                Some("tool_use") if role == "assistant" => {
                    let input = block.get("input").cloned();
                    parts.push(TranscriptPart {
                        kind: "toolCall".into(),
                        name: block.get("name").and_then(Value::as_str).map(str::to_owned),
                        arguments: input,
                        tool_call_id: block.get("id").and_then(Value::as_str).map(str::to_owned),
                        ..Default::default()
                    });
                }
                Some("image") if role == "user" => {
                    let source = block.get("source").unwrap_or(&Value::Null);
                    let mime = source.get("media_type").and_then(Value::as_str);
                    if let Some(mime) = mime.filter(|mime| allowed_image_mime(mime)) {
                        parts.push(TranscriptPart {
                            kind: "image".into(),
                            mime_type: Some(mime.to_owned()),
                            image_index: Some(index),
                            ..Default::default()
                        });
                    }
                }
                Some("tool_result") if role == "user" => {
                    let result_text = content_text(block.get("content"));
                    let tool_call_id = block
                        .get("tool_use_id")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    let tool_name = block.get("name").and_then(Value::as_str).map(str::to_owned);
                    let is_error = block.get("is_error").and_then(Value::as_bool);
                    let mut result = empty_message(
                        format!("{id}-tool-{index}"),
                        "toolResult",
                        result_text.clone(),
                        created_at.clone(),
                        if result_text.is_empty() {
                            Vec::new()
                        } else {
                            vec![TranscriptPart {
                                kind: "text".into(),
                                text: Some(result_text),
                                ..Default::default()
                            }]
                        },
                    );
                    result.tool_call_id = tool_call_id;
                    result.tool_name = tool_name;
                    result.is_error = is_error;
                    messages.push(result);
                }
                _ => {}
            }
        }

        if !text.is_empty() || !parts.is_empty() {
            let mut entry = empty_message(id, role, text, created_at, parts);
            entry.model = model;
            entry.usage = usage;
            messages.push(entry);
        }
    }
    messages
}

const MAX_IMAGE_BASE64_BYTES: usize = 12 * 1024 * 1024;

fn allowed_image_mime(mime: &str) -> bool {
    matches!(mime, "image/png" | "image/jpeg" | "image/gif" | "image/webp" | "image/bmp" | "image/avif")
}

fn decode_base64(input: &str) -> Option<Vec<u8>> {
    let mut output = Vec::with_capacity(input.len() / 4 * 3);
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for byte in input.bytes() {
        if byte == b'=' || byte.is_ascii_whitespace() { continue; }
        let value = match byte {
            b'A'..=b'Z' => (byte - b'A') as u32,
            b'a'..=b'z' => (byte - b'a') as u32 + 26,
            b'0'..=b'9' => (byte - b'0') as u32 + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        buffer = (buffer << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Some(output)
}

/// Read one base64 image block from Claude's native JSONL transcript.
pub fn read_image(session_id: &str, entry_id: &str, index: usize) -> Option<(String, Vec<u8>)> {
    if session_id.is_empty() || !session_id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
        return None;
    }
    let path = find_transcript(&projects_root(), session_id)?;
    let metadata = fs::metadata(&path).ok()?;
    if metadata.len() > MAX_TRANSCRIPT_BYTES { return None; }
    let file = fs::File::open(path).ok()?;
    for line in BufReader::new(file).lines().take(MAX_TRANSCRIPT_LINES) {
        let line = line.ok()?;
        let record: Value = serde_json::from_str(&line).ok()?;
        let id = record.get("uuid").and_then(Value::as_str)
            .or_else(|| record.pointer("/message/id").and_then(Value::as_str));
        if id != Some(entry_id) { continue; }
        let block = record.pointer("/message/content")?.as_array()?.get(index)?;
        if block.get("type").and_then(Value::as_str) != Some("image") { return None; }
        let source = block.get("source")?;
        if source.get("type").and_then(Value::as_str) != Some("base64") { return None; }
        let mime = source.get("media_type").and_then(Value::as_str)
            .filter(|mime| allowed_image_mime(mime))?;
        let data = source.get("data").and_then(Value::as_str)?;
        if data.len() > MAX_IMAGE_BASE64_BYTES { return None; }
        let bytes = decode_base64(data)?;
        if bytes.len() > MAX_IMAGE_BASE64_BYTES { return None; }
        return Some((mime.to_owned(), bytes));
    }
    None
}

fn append_text(text: &mut String, parts: &mut Vec<TranscriptPart>, value: &str) {
    text.push_str(value);
    parts.push(TranscriptPart {
        kind: "text".into(),
        text: Some(value.to_owned()),
        ..Default::default()
    });
}

fn content_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|block| {
                block
                    .get("text")
                    .and_then(Value::as_str)
                    .or_else(|| block.as_str())
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Some(Value::Object(object)) => object
            .get("stdout")
            .and_then(Value::as_str)
            .or_else(|| object.get("stderr").and_then(Value::as_str))
            .unwrap_or_default()
            .to_owned(),
        _ => String::new(),
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

    #[test]
    fn parses_claude_messages_tools_and_usage() {
        let source = concat!(
            "{\"type\":\"user\",\"uuid\":\"u1\",\"timestamp\":\"2026-11-03T10:00:00Z\",\"message\":{\"role\":\"user\",\"content\":\"hello\"}}\n",
            "{\"type\":\"assistant\",\"uuid\":\"a1\",\"message\":{\"role\":\"assistant\",\"model\":\"claude-sonnet\",\"usage\":{\"input_tokens\":12},\"content\":[{\"type\":\"text\",\"text\":\"checking\"},{\"type\":\"tool_use\",\"id\":\"tool-1\",\"name\":\"Bash\",\"input\":{\"command\":\"pwd\"}}]}}\n",
            "{\"type\":\"user\",\"uuid\":\"u2\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"tool-1\",\"content\":[{\"type\":\"text\",\"text\":\"/workspace\"}]}]}}\n",
            "{\"type\":\"assistant\",\"uuid\":\"side\",\"isSidechain\":true,\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"not main thread\"}]}}\n"
        );
        let messages = parse_reader(BufReader::new(source.as_bytes()));
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].content, "hello");
        assert_eq!(messages[1].content, "checking");
        assert_eq!(messages[1].model.as_deref(), Some("claude-sonnet"));
        assert_eq!(
            messages[1].parts.as_ref().unwrap()[1].name.as_deref(),
            Some("Bash")
        );
        assert_eq!(messages[2].role, "toolResult");
        assert_eq!(messages[2].tool_call_id.as_deref(), Some("tool-1"));
        assert_eq!(messages[2].content, "/workspace");
    }

    #[test]
    fn rejects_session_ids_that_could_escape_the_project_directory() {
        assert!(read_messages("../outside").is_empty());
        assert!(read_messages("nested/session").is_empty());
    }

    #[test]
    fn skips_malformed_partial_lines_and_sidechains() {
        let source = concat!(
            "{\"type\":\"assistant\",\"uuid\":\"bad\",\"message\":}\n",
            "{\"type\":\"assistant\",\"uuid\":\"side\",\"isSidechain\":true,\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"hidden\"}]}}\n"
        );
        assert!(parse_reader(BufReader::new(source.as_bytes())).is_empty());
    }
}
