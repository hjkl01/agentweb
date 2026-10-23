//! Read chat messages from a pi native session transcript.
//!
//! pi stores each session as JSONL under `$PI_CODING_AGENT_DIR/sessions` (or
//! `$HOME/.pi/agent/sessions`), one directory per working directory. Every
//! `message` entry carries the provider message that agentweb renders.
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    io::Read,
    path::{Path, PathBuf},
};

/// One structured element of a message's content array. `text` carries the
/// body of text/thinking parts; toolCall parts carry `name`/`arguments`.
#[derive(Serialize, Default)]
pub struct TranscriptPart {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Present on `image` parts: the media type and the index of the part in
    /// the original content array. The bytes are fetched on demand from
    /// [`read_entry_image`] so base64 never bloats the message list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_index: Option<usize>,
}

#[derive(Serialize)]
pub struct TranscriptMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// Structured content for assistant/toolResult messages. `content` stays the
    /// concatenated visible text so older clients keep working.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parts: Option<Vec<TranscriptPart>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
    /// Token/cost usage reported by Pi. Present on assistant turns and on tool
    /// results that performed nested model work.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Value>,
    /// Present on `toolResult` messages: which tool ran, how long it took and
    /// whether Pi cut the output short (`details.truncation`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
    /// Line-numbered change preview Pi attaches to `edit` results
    /// (`details.diff`: `" 12 context"`, `"-13 old"`, `"+13 new"`), so the
    /// transcript can show what changed without re-reading the file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
    /// The same change as a real unified diff (`details.patch`, with `---`/`+++`
    /// and `@@` headers), which is what a side-by-side view can parse. Pi sends
    /// both keys, and neither is guaranteed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patch: Option<String>,
    /// Present on assistant messages: the answering model, why the turn stopped
    /// (`stop`/`toolUse`/`error`/`length`) and the provider error if it failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    /// Raw `details` map from the Pi toolResult message (contains `server`/`tool`
    /// for MCP tools, plus diff/patch/truncation already extracted above).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
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

#[derive(Clone, Debug)]
pub struct NativeSessionSummary {
    pub id: String,
    pub cwd: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
}

/// A discovery result is only authoritative when every relevant directory was
/// scanned. Callers must not infer that sessions are deleted from an incomplete
/// scan (for example, because a directory is unreadable or a safety limit hit).
#[derive(Clone, Debug)]
pub struct NativeSessionDiscovery {
    pub sessions: Vec<NativeSessionSummary>,
    pub complete: bool,
}

/// Discover native Pi sessions without persisting transcript contents in SQLite.
pub fn list_native_sessions() -> NativeSessionDiscovery {
    let Some(root) = sessions_root() else {
        return NativeSessionDiscovery {
            sessions: Vec::new(),
            complete: false,
        };
    };
    let root_meta = match std::fs::symlink_metadata(&root) {
        Ok(metadata) if !metadata.file_type().is_symlink() && metadata.is_dir() => metadata,
        _ => {
            return NativeSessionDiscovery {
                sessions: Vec::new(),
                complete: false,
            }
        }
    };
    let _ = root_meta;

    const MAX_ENTRIES: usize = 20_000;
    const MAX_DEPTH: usize = 32;
    const MAX_HEADER_BYTES: usize = 64 * 1024;
    const MAX_TITLE_SCAN_BYTES: usize = 256 * 1024;
    let mut pending = vec![(root, 0usize)];
    let mut scanned = 0usize;
    let mut complete = true;
    let mut sessions = Vec::new();
    while let Some((dir, depth)) = pending.pop() {
        if depth > MAX_DEPTH || scanned >= MAX_ENTRIES {
            complete = false;
            break;
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => {
                complete = false;
                continue;
            }
        };
        for entry in entries {
            scanned += 1;
            if scanned > MAX_ENTRIES {
                complete = false;
                break;
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    complete = false;
                    continue;
                }
            };
            let path = entry.path();
            let metadata = match std::fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(_) => {
                    complete = false;
                    continue;
                }
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                if depth < MAX_DEPTH {
                    pending.push((path, depth + 1));
                } else {
                    complete = false;
                }
                continue;
            }
            if !metadata.is_file() || path.extension().and_then(|v| v.to_str()) != Some("jsonl") {
                continue;
            }
            let file = match std::fs::File::open(&path) {
                Ok(file) => file,
                Err(_) => {
                    complete = false;
                    continue;
                }
            };
            let mut reader = std::io::BufReader::new(file);
            let mut first_line = String::new();
            if std::io::BufRead::read_line(&mut reader, &mut first_line).is_err()
                || first_line.len() > MAX_HEADER_BYTES
            {
                complete = false;
                continue;
            }
            let Ok(header) = serde_json::from_str::<Value>(&first_line) else {
                continue;
            };
            if header.get("type").and_then(Value::as_str) != Some("session") {
                continue;
            }
            let Some(id) = header.get("id").and_then(Value::as_str) else {
                continue;
            };
            let Some(cwd) = header.get("cwd").and_then(Value::as_str) else {
                continue;
            };
            if id.is_empty() || cwd.is_empty() {
                continue;
            }

            let mut title = String::new();
            let mut scanned_bytes = first_line.len();
            while scanned_bytes < MAX_TITLE_SCAN_BYTES {
                let mut line = String::new();
                let read = match std::io::BufRead::read_line(&mut reader, &mut line) {
                    Ok(read) => read,
                    Err(_) => {
                        complete = false;
                        break;
                    }
                };
                if read == 0 {
                    break;
                }
                scanned_bytes += read;
                let Ok(entry) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if entry.get("type").and_then(Value::as_str) != Some("message") {
                    continue;
                }
                let Some(message) = entry.get("message") else {
                    continue;
                };
                if message.get("role").and_then(Value::as_str) != Some("user") {
                    continue;
                };
                if let Some(text) = message.get("content").and_then(text_content) {
                    title = text.split_whitespace().collect::<Vec<_>>().join(" ");
                    if !title.is_empty() {
                        break;
                    }
                }
            }
            if scanned_bytes >= MAX_TITLE_SCAN_BYTES {
                complete = false;
            }
            if title.is_empty() {
                title = format!("Pi session {}", id.chars().take(8).collect::<String>());
            }
            title = title.chars().take(80).collect();
            let timestamp = header
                .get("timestamp")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    metadata
                        .modified()
                        .ok()
                        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|duration| {
                            chrono::DateTime::<chrono::Utc>::from_timestamp(
                                duration.as_secs() as i64,
                                0,
                            )
                            .map(|value| value.to_rfc3339())
                            .unwrap_or_default()
                        })
                        .unwrap_or_default()
                });
            let updated_at = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .and_then(|duration| {
                    chrono::DateTime::<chrono::Utc>::from_timestamp(duration.as_secs() as i64, 0)
                })
                .map(|value| value.to_rfc3339())
                .unwrap_or_else(|| timestamp.clone());
            sessions.push(NativeSessionSummary {
                id: id.to_owned(),
                cwd: cwd.to_owned(),
                title,
                created_at: timestamp,
                updated_at,
            });
        }
        if scanned > MAX_ENTRIES {
            break;
        }
    }
    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    NativeSessionDiscovery { sessions, complete }
}

/// A failed or incomplete filesystem lookup is not evidence that a session was deleted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeSessionFileLookup {
    Found,
    Missing,
    Unknown,
}

/// Return whether a Pi session exists in the Agent's native session store.
/// Unknown means the filesystem could not be scanned reliably.
pub fn native_session_file_exists(native_session_id: &str) -> NativeSessionFileLookup {
    let Some(root) = sessions_root() else {
        return NativeSessionFileLookup::Unknown;
    };
    match find_by_suffix(
        &root,
        &format!("_{native_session_id}.jsonl"),
        native_session_id,
    ) {
        Ok(Some(_)) => NativeSessionFileLookup::Found,
        Ok(None) => NativeSessionFileLookup::Missing,
        Err(()) => NativeSessionFileLookup::Unknown,
    }
}

/// Read the native transcript from a session-specific workspace state directory.
/// The backend process cannot see environment variables assigned only to the
/// spawned Pi child, so API handlers should pass the persisted workspace path.
#[cfg(test)]
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

/// Return whether a valid Pi transcript file exists in the workspace-scoped
/// session directory. This lets API callers distinguish a deleted native
/// session from a valid transcript that simply contains no visible messages.
#[cfg(test)]
pub fn native_session_file_exists_from_workspace(
    workspace: &Path,
    native_session_id: &str,
) -> NativeSessionFileLookup {
    let Ok(workspace) = workspace.canonicalize() else {
        return NativeSessionFileLookup::Unknown;
    };
    let state_dir = workspace.join(".agentweb-state");
    let session_dir = state_dir.join("pi-sessions");
    for path in [&state_dir, &session_dir] {
        let metadata = match std::fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(_) => return NativeSessionFileLookup::Unknown,
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return NativeSessionFileLookup::Unknown;
        }
    }
    match find_by_suffix(
        &session_dir,
        &format!("_{native_session_id}.jsonl"),
        native_session_id,
    ) {
        Ok(Some(_)) => NativeSessionFileLookup::Found,
        Ok(None) => NativeSessionFileLookup::Missing,
        Err(()) => NativeSessionFileLookup::Unknown,
    }
}

fn read_messages_from_root(root: &Path, native_session_id: &str) -> Vec<TranscriptMessage> {
    let suffix = format!("_{native_session_id}.jsonl");
    let Ok(Some(path)) = find_by_suffix(root, &suffix, native_session_id) else {
        return Vec::new();
    };
    let Some(content) = read_transcript_text(&path) else {
        return Vec::new();
    };
    parse_messages(&content, native_session_id)
}

/// Read a whole transcript file with a bounded buffer. A partial final UTF-8
/// sequence or JSON object should not hide prior messages, so lossy decoding
/// lets the parser skip only the incomplete tail.
fn read_transcript_text(path: &Path) -> Option<String> {
    // Keep a single transcript refresh from allocating unbounded memory.
    // Typical conversations are far smaller; oversized/corrupt files fail
    // closed instead of blocking the API worker with a huge JSONL parse.
    const MAX_TRANSCRIPT_BYTES: u64 = 64 * 1024 * 1024;
    // Open once, then inspect and read from that same handle. This avoids a
    // path-based metadata/read race where the candidate could be replaced
    // between the size check and the read.
    let file = std::fs::File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > MAX_TRANSCRIPT_BYTES {
        return None;
    }
    // Bound the actual read too: the file may grow after metadata() returns.
    let mut bytes = Vec::new();
    if file
        .take(MAX_TRANSCRIPT_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() as u64 > MAX_TRANSCRIPT_BYTES
    {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn find_by_suffix(
    dir: &Path,
    suffix: &str,
    native_session_id: &str,
) -> Result<Option<PathBuf>, ()> {
    // Bound traversal and never follow symlinks. A failed directory or entry
    // read makes the result unknown; it must not be treated as a deleted session.
    const MAX_ENTRIES: usize = 20_000;
    const MAX_DEPTH: usize = 32;
    let root_meta = std::fs::symlink_metadata(dir).map_err(|_| ())?;
    if root_meta.file_type().is_symlink() || !root_meta.is_dir() {
        return Err(());
    }

    let mut pending = vec![(dir.to_path_buf(), 0usize)];
    let mut scanned = 0usize;
    while let Some((current, depth)) = pending.pop() {
        if depth > MAX_DEPTH || scanned >= MAX_ENTRIES {
            return Err(());
        }
        let entries = std::fs::read_dir(&current).map_err(|_| ())?;
        for entry in entries {
            let entry = entry.map_err(|_| ())?;
            scanned += 1;
            if scanned > MAX_ENTRIES {
                return Err(());
            }
            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path).map_err(|_| ())?;
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                if depth < MAX_DEPTH {
                    pending.push((path, depth + 1));
                } else {
                    return Err(());
                }
            } else if metadata.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with(suffix))
            {
                let file = match std::fs::File::open(&path) {
                    Ok(file) => file,
                    Err(_) => return Err(()),
                };
                const MAX_HEADER_BYTES: u64 = 64 * 1024;
                let mut reader = std::io::BufReader::new(file).take(MAX_HEADER_BYTES);
                let mut first_line = String::new();
                if std::io::BufRead::read_line(&mut reader, &mut first_line).is_err() {
                    return Err(());
                }
                if first_line.len() as u64 >= MAX_HEADER_BYTES {
                    // Ignore an oversized candidate and continue: another file
                    // with the same suffix may still be the valid session.
                    continue;
                }
                let Ok(header) = serde_json::from_str::<Value>(&first_line) else {
                    // A malformed duplicate filename is not authoritative.
                    continue;
                };
                if header.get("type").and_then(Value::as_str) == Some("session")
                    && header.get("id").and_then(Value::as_str) == Some(native_session_id)
                {
                    return Ok(Some(path));
                }
                // A matching filename with a different header can be stale or
                // corrupt; continue scanning for a valid duplicate candidate.
            }
        }
    }
    Ok(None)
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

/// Parse an assistant/toolResult content array into structured parts. Returns
/// None for plain-string content so the caller can keep using `content`.
fn message_parts(content: &Value) -> Option<Vec<TranscriptPart>> {
    let Value::Array(items) = content else {
        return None;
    };
    let mut parts = Vec::new();
    for (index, item) in items.iter().enumerate() {
        match item.get("type").and_then(Value::as_str).unwrap_or_default() {
            "text" => {
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    if !text.is_empty() {
                        parts.push(TranscriptPart {
                            kind: "text".into(),
                            text: Some(text.into()),
                            ..Default::default()
                        });
                    }
                }
            }
            "thinking" | "reasoning" => {
                if let Some(text) = item
                    .get("thinking")
                    .or_else(|| item.get("text"))
                    .and_then(Value::as_str)
                {
                    if !text.is_empty() {
                        parts.push(TranscriptPart {
                            kind: "thinking".into(),
                            text: Some(text.into()),
                            ..Default::default()
                        });
                    }
                }
            }
            "toolCall" | "tool_call" | "tool_use" => {
                let name = item
                    .get("name")
                    .or_else(|| item.get("toolName"))
                    .and_then(Value::as_str)
                    .unwrap_or("tool");
                parts.push(TranscriptPart {
                    kind: "toolCall".into(),
                    name: Some(name.into()),
                    arguments: item.get("arguments").or_else(|| item.get("input")).cloned(),
                    tool_call_id: item
                        .get("id")
                        .or_else(|| item.get("toolCallId"))
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    ..Default::default()
                });
            }
            "image" => {
                let mime = item
                    .get("mimeType")
                    .or_else(|| item.get("mime_type"))
                    .and_then(Value::as_str);
                if let Some(mime) = mime.filter(|mime| allowed_image_mime(mime)) {
                    parts.push(TranscriptPart {
                        kind: "image".into(),
                        mime_type: Some(mime.into()),
                        image_index: Some(index),
                        ..Default::default()
                    });
                }
            }
            _ => {}
        }
    }
    (!parts.is_empty()).then_some(parts)
}

/// Cap for the `edit` diff we ship with a tool result: it exists to explain a
/// change in place, not to duplicate whole files.
const MAX_RESULT_DIFF_CHARS: usize = 200_000;

fn message_tool_call_id(message: &Value) -> Option<String> {
    message
        .get("toolCallId")
        .or_else(|| message.get("tool_call_id"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn message_text_field(message: &Value, key: &str) -> Option<String> {
    message
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn message_details(message: &Value) -> Option<&Value> {
    message.get("details").filter(|value| !value.is_null())
}

/// Text Pi puts under `details.<key>` on a tool result. Capped so a single
/// patch cannot blow up the transcript payload.
fn message_result_details_text(message: &Value, key: &str) -> Option<String> {
    message_details(message)
        .and_then(|details| details.get(key))
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty() && text.chars().count() <= MAX_RESULT_DIFF_CHARS)
        .map(str::to_owned)
}

fn message_is_error(message: &Value) -> Option<bool> {
    message
        .get("isError")
        .or_else(|| message.get("is_error"))
        .and_then(Value::as_bool)
}

pub fn read_messages(native_session_id: &str) -> Vec<TranscriptMessage> {
    let Some(root) = sessions_root() else {
        return Vec::new();
    };
    read_messages_from_root(&root, native_session_id)
}

/// One message hit inside a transcript, with a snippet for display.
#[derive(Clone, Debug)]
pub struct TranscriptSearchHit {
    pub session_id: String,
    pub entry_id: String,
    pub role: String,
    pub before: String,
    pub matched: String,
    pub after: String,
}

/// Search a bounded number of transcripts for a literal, case-insensitive needle.
///
/// `native_session_ids` must be ordered newest first: the caps apply in that order, so a
/// bounded search still covers the sessions a user is most likely to mean. The returned flag
/// says the result is incomplete (a cap or the time budget was hit, or a file was unreadable)
/// — never that older sessions have no match.
// ponytail: scan transcript files without an index; add an index if measured latency warrants it.
pub fn search_transcripts(
    native_session_ids: &[String],
    needle: &str,
    budget: std::time::Duration,
) -> (Vec<TranscriptSearchHit>, bool) {
    let Some(root) = sessions_root() else {
        return (Vec::new(), true);
    };
    search_transcripts_in_root(&root, native_session_ids, needle, budget)
}

fn search_transcripts_in_root(
    root: &Path,
    native_session_ids: &[String],
    needle: &str,
    budget: std::time::Duration,
) -> (Vec<TranscriptSearchHit>, bool) {
    const MAX_FILES: usize = 300;
    const MAX_RESULTS: usize = 30;
    let mut hits = Vec::new();
    let mut truncated = false;
    let deadline = std::time::Instant::now() + budget;
    let needle = needle.to_lowercase();
    let files = session_files(root);
    for (position, native_session_id) in native_session_ids.iter().enumerate() {
        if hits.len() >= MAX_RESULTS
            || position >= MAX_FILES
            || std::time::Instant::now() >= deadline
        {
            truncated = true;
            break;
        }
        let Some(path) = files.get(native_session_id.as_str()) else {
            continue;
        };
        let Some(content) = read_search_text(path) else {
            truncated = true;
            continue;
        };
        if !transcript_belongs_to(&content, native_session_id) {
            continue;
        }
        let (hit, gave_up) = first_transcript_match(&content, native_session_id, &needle, deadline);
        if let Some(hit) = hit {
            hits.push(hit);
        }
        if gave_up {
            truncated = true;
            break;
        }
    }
    (hits, truncated)
}

/// Map every `<timestamp>_<session id>.jsonl` in the store to its path. The filename carries
/// the session id, so this walk opens no files; the header is checked when a file is read.
fn session_files(root: &Path) -> HashMap<String, PathBuf> {
    const MAX_ENTRIES: usize = 20_000;
    const MAX_DEPTH: usize = 32;
    let mut files = HashMap::new();
    let Ok(root_meta) = std::fs::symlink_metadata(root) else {
        return files;
    };
    if root_meta.file_type().is_symlink() || !root_meta.is_dir() {
        return files;
    }
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    let mut scanned = 0usize;
    while let Some((dir, depth)) = pending.pop() {
        if depth > MAX_DEPTH || scanned >= MAX_ENTRIES {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            scanned += 1;
            if scanned > MAX_ENTRIES {
                break;
            }
            let path = entry.path();
            let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                if depth < MAX_DEPTH {
                    pending.push((path, depth + 1));
                }
                continue;
            }
            if !metadata.is_file() {
                continue;
            }
            let Some(id) = session_id_from_name(&path) else {
                continue;
            };
            files.entry(id.to_owned()).or_insert(path);
        }
    }
    files
}

fn session_id_from_name(path: &Path) -> Option<&str> {
    let name = path.file_name()?.to_str()?;
    let stem = name.strip_suffix(".jsonl")?;
    let (_, id) = stem.rsplit_once('_')?;
    (!id.is_empty()).then_some(id)
}

/// Cap what one file can contribute to a search; a larger transcript is only partly searched.
const SEARCH_FILE_BYTES: u64 = 16 * 1024 * 1024;

fn read_search_text(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(SEARCH_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// A filename match is not enough: the header names the session that owns the file.
fn transcript_belongs_to(content: &str, native_session_id: &str) -> bool {
    let Some(first_line) = content.lines().next() else {
        return false;
    };
    let Ok(header) = serde_json::from_str::<Value>(first_line) else {
        return false;
    };
    header.get("type").and_then(Value::as_str) == Some("session")
        && header.get("id").and_then(Value::as_str) == Some(native_session_id)
}

/// The first user/assistant message containing the needle, plus whether the budget ran out.
fn first_transcript_match(
    content: &str,
    native_session_id: &str,
    needle: &str,
    deadline: std::time::Instant,
) -> (Option<TranscriptSearchHit>, bool) {
    for message in parse_messages(content, native_session_id) {
        if message.role != "user" && message.role != "assistant" {
            continue;
        }
        if std::time::Instant::now() >= deadline {
            return (None, true);
        }
        let Some((start, end)) = find_ci(&message.content, needle) else {
            continue;
        };
        let (before, matched, after) = snippet(&message.content, start, end);
        return (
            Some(TranscriptSearchHit {
                session_id: native_session_id.to_owned(),
                entry_id: message.id,
                role: message.role,
                before,
                matched,
                after,
            }),
            false,
        );
    }
    (None, false)
}

/// Byte range of the first case-insensitive occurrence of `needle` (already lowercased).
fn find_ci(haystack: &str, needle: &str) -> Option<(usize, usize)> {
    if needle.is_empty() {
        return None;
    }
    let mut start = haystack.to_lowercase().find(needle)?;
    // Lowercasing can change a character's byte length; keep the range on char boundaries.
    while start > 0 && !haystack.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = start;
    for _ in 0..needle.chars().count() {
        match haystack[end..].chars().next() {
            Some(character) => end += character.len_utf8(),
            None => break,
        }
    }
    Some((start, end))
}

const SEARCH_SNIPPET_CHARS: usize = 60;

fn snippet(text: &str, start: usize, end: usize) -> (String, String, String) {
    let window_start = text[..start]
        .char_indices()
        .rev()
        .nth(SEARCH_SNIPPET_CHARS - 1)
        .map(|(index, _)| index)
        .unwrap_or(0);
    let window_end = text[end..]
        .char_indices()
        .nth(SEARCH_SNIPPET_CHARS)
        .map(|(index, _)| end + index)
        .unwrap_or(text.len());
    let flatten = |value: &str| value.split_whitespace().collect::<Vec<_>>().join(" ");
    let before = flatten(&text[window_start..start]);
    let after = flatten(&text[end..window_end]);
    (
        if window_start > 0 {
            format!("…{before}")
        } else {
            before
        },
        text[start..end].to_owned(),
        if window_end < text.len() {
            format!("{after}…")
        } else {
            after
        },
    )
}

/// Media types safe to render inline from a transcript part. Raster only:
/// SVG can carry scripts when opened directly, so it is not served here.
fn allowed_image_mime(mime: &str) -> bool {
    matches!(
        mime,
        "image/png"
            | "image/jpeg"
            | "image/gif"
            | "image/webp"
            | "image/bmp"
            | "image/avif"
            | "image/x-icon"
    )
}

/// Cap a single inline image; screenshots are normally far smaller.
const MAX_IMAGE_BASE64_BYTES: usize = 12 * 1024 * 1024;

fn decode_base64(input: &str) -> Option<Vec<u8>> {
    fn value(byte: u8) -> Option<u32> {
        Some(match byte {
            b'A'..=b'Z' => (byte - b'A') as u32,
            b'a'..=b'z' => (byte - b'a') as u32 + 26,
            b'0'..=b'9' => (byte - b'0') as u32 + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        })
    }
    let mut out = Vec::with_capacity(input.len() / 4 * 3);
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for byte in input.bytes() {
        if byte == b'=' || byte == b'\n' || byte == b'\r' || byte == b' ' {
            continue;
        }
        buffer = (buffer << 6) | value(byte)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// Return the `(mime, bytes)` of an image content part stored in a native
/// transcript entry. Base64 is decoded here so the message list never carries
/// inline image payloads.
pub fn read_entry_image(
    native_session_id: &str,
    entry_id: &str,
    index: usize,
) -> Option<(String, Vec<u8>)> {
    let root = sessions_root()?;
    read_entry_image_from_root(&root, native_session_id, entry_id, index)
}

fn read_entry_image_from_root(
    root: &Path,
    native_session_id: &str,
    entry_id: &str,
    index: usize,
) -> Option<(String, Vec<u8>)> {
    let suffix = format!("_{native_session_id}.jsonl");
    let path = find_by_suffix(root, &suffix, native_session_id).ok()??;
    let content = read_transcript_text(&path)?;
    // Match `parse_messages`: only trust a file whose header names this session.
    let header = serde_json::from_str::<Value>(content.lines().next()?).ok()?;
    if header.get("type").and_then(Value::as_str) != Some("session")
        || header.get("id").and_then(Value::as_str) != Some(native_session_id)
    {
        return None;
    }
    for line in content.lines() {
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if entry.get("id").and_then(Value::as_str) != Some(entry_id) {
            continue;
        }
        let item = entry
            .get("message")?
            .get("content")?
            .as_array()?
            .get(index)?;
        if item.get("type").and_then(Value::as_str) != Some("image") {
            return None;
        }
        let mime = item
            .get("mimeType")
            .or_else(|| item.get("mime_type"))
            .and_then(Value::as_str)
            .filter(|mime| allowed_image_mime(mime))?;
        let data = item.get("data").and_then(Value::as_str)?;
        if data.len() > MAX_IMAGE_BASE64_BYTES {
            return None;
        }
        return Some((mime.to_owned(), decode_base64(data)?));
    }
    None
}

fn parse_messages(content: &str, native_session_id: &str) -> Vec<TranscriptMessage> {
    // The first JSONL record is the session header. Do not let a malformed or
    // missing header be silently skipped so a later record can masquerade as
    // the header; malformed records after the header are still ignored below.
    let Some(first_line) = content.lines().next() else {
        return Vec::new();
    };
    let Ok(header) = serde_json::from_str::<Value>(first_line) else {
        return Vec::new();
    };
    if header.get("type").and_then(Value::as_str) != Some("session")
        || header.get("id").and_then(Value::as_str) != Some(native_session_id)
    {
        return Vec::new();
    }

    // A byte limit alone does not bound the number of allocated JSON
    // values: a transcript can contain many tiny records. Cap valid entries
    // before building the tree index to keep per-refresh memory predictable.
    const MAX_TRANSCRIPT_ENTRIES: usize = 100_000;
    // Malformed lines still cost CPU to parse, even though they do not count
    // toward the valid-entry limit. Count the header as a line too.
    const MAX_TRANSCRIPT_LINES: usize = 200_000;
    let mut entries = Vec::new();
    for (index, line) in content.lines().enumerate() {
        if index >= MAX_TRANSCRIPT_LINES {
            return Vec::new();
        }
        if let Ok(entry) = serde_json::from_str::<Value>(line) {
            if entries.len() >= MAX_TRANSCRIPT_ENTRIES {
                return Vec::new();
            }
            entries.push((index, entry));
        }
    }

    // A filename match is not enough: only render a transcript whose
    // session header identifies the native session requested by the database.
    // This also prevents a malformed or replaced file from being shown as a
    // different conversation.
    let Some((_, header)) = entries.first() else {
        return Vec::new();
    };
    if header.get("type").and_then(Value::as_str) != Some("session")
        || header.get("id").and_then(Value::as_str) != Some(native_session_id)
    {
        return Vec::new();
    }

    // Pi v2/v3 sessions are trees. On load, Pi restores the leaf from the last
    // persisted entry, then derives the visible branch by following parentId.
    // Older linear sessions have no parentId fields and remain readable as-is.
    let has_tree_links = entries
        .iter()
        .any(|(_, entry)| entry.get("parentId").is_some());
    let active_ids = if has_tree_links {
        let mut by_id = std::collections::HashMap::new();
        for (_, entry) in &entries {
            let Some(id) = entry.get("id").and_then(Value::as_str) else {
                continue;
            };
            // IDs identify nodes in Pi's session tree. A duplicate makes
            // parent traversal ambiguous, so fail closed rather than choosing
            // an arbitrary branch based on JSONL ordering.
            if by_id.insert(id.to_owned(), entry).is_some() {
                return Vec::new();
            }
        }
        let mut active = std::collections::HashSet::new();
        let mut current = entries.iter().rev().find_map(|(_, entry)| {
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
        let entry_type = entry
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        // A compaction replaces earlier context with a summary; surface it so
        // the transcript shows where history was folded instead of hiding it.
        if entry_type == "compaction" {
            let summary = entry
                .get("summary")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if summary.is_empty() {
                continue;
            }
            messages.push(TranscriptMessage {
                id: entry
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("{native_session_id}-{index}")),
                role: "compaction".into(),
                content: summary.to_owned(),
                created_at: entry
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                parts: None,
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
                details: None,
            });
            continue;
        }
        if entry_type != "message" {
            continue;
        }
        let Some(message) = entry.get("message") else {
            continue;
        };
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if role != "user" && role != "assistant" && role != "toolResult" {
            continue;
        }
        let content = message.get("content");
        let text = content.and_then(text_content).unwrap_or_default();
        let parts = content.and_then(message_parts);
        let is_tool_result = role == "toolResult";
        let is_assistant = role == "assistant";
        // A provider error arrives as an assistant turn with empty content, so
        // keep it: without the entry the transcript just loses the turn.
        let error_message = is_assistant
            .then(|| message_text_field(message, "errorMessage"))
            .flatten();
        let truncated = is_tool_result
            && message_details(message).is_some_and(|d| d.get("truncation").is_some());
        // Keep a tool-only assistant turn (no text) and an empty tool result;
        // drop entries that carry nothing renderable at all.
        if text.is_empty() && parts.is_none() && error_message.is_none() {
            continue;
        }
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
            parts,
            tool_call_id: is_tool_result
                .then(|| message_tool_call_id(message))
                .flatten(),
            is_error: is_tool_result.then(|| message_is_error(message)).flatten(),
            usage: message.get("usage").cloned(),
            tool_name: is_tool_result
                .then(|| message_text_field(message, "toolName"))
                .flatten(),
            duration_ms: message.get("durationMs").and_then(Value::as_u64),
            truncated: truncated.then_some(true),
            diff: is_tool_result
                .then(|| message_result_details_text(message, "diff"))
                .flatten(),
            patch: is_tool_result
                .then(|| message_result_details_text(message, "patch"))
                .flatten(),
            model: is_assistant
                .then(|| message_text_field(message, "model"))
                .flatten(),
            stop_reason: is_assistant
                .then(|| message_text_field(message, "stopReason"))
                .flatten(),
            error_message,
            details: is_tool_result
                .then(|| message_details(message).cloned())
                .flatten(),
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
    fn finds_matching_session_when_duplicate_filename_is_invalid() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pi-transcript-duplicate-{unique}"));
        let first = dir.join("a");
        let second = dir.join("b");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        let filename = "2026-01-01T00-00-00-000Z_same-id.jsonl";
        std::fs::write(
            first.join(filename),
            r#"{"type":"session","id":"not-same-id"}"#,
        )
        .unwrap();
        std::fs::write(
            second.join(filename),
            [
                r#"{"type":"session","id":"same-id"}"#,
                r#"{"type":"message","id":"m1","parentId":null,"message":{"role":"user","content":"correct session"}}"#,
            ]
            .join("\n"),
        )
        .unwrap();

        let path = find_by_suffix(&dir, "_same-id.jsonl", "same-id")
            .unwrap()
            .unwrap();
        let messages = parse_messages(&std::fs::read_to_string(path).unwrap(), "same-id");
        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "correct session");
    }

    #[test]
    fn rejects_header_larger_than_header_limit() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pi-transcript-long-header-{unique}"));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("2026-01-01T00-00-00-000Z_large-id.jsonl");
        let mut content = String::from(r#"{"type":"session","id":"large-id","padding":""#);
        content.push_str(&"x".repeat(70 * 1024));
        content.push_str(r#""}"#);
        std::fs::write(&file, content).unwrap();

        assert!(matches!(
            find_by_suffix(&dir, "_large-id.jsonl", "large-id"),
            Ok(None) | Err(())
        ));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn preserves_complete_messages_when_last_jsonl_entry_is_incomplete() {
        let content = [
            r#"{"type":"session","id":"session-tail","version":3}"#,
            r#"{"type":"message","id":"u1","parentId":null,"message":{"role":"user","content":"complete message"}}"#,
            r#"{"type":"message","id":"a1","parentId":"u1","message":{"role":"assistant","content":"partial"#,
        ]
        .join("\n");

        let messages = parse_messages(&content, "session-tail");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "complete message");
    }

    #[test]
    fn rejects_transcript_with_too_many_entries() {
        let mut lines = vec![r#"{"type":"session","id":"session-limit","version":3}"#.to_owned()];
        for index in 0..100_000 {
            lines.push(format!(r#"{{"type":"custom","id":"entry-{index}"}}"#));
        }
        let content = lines.join("\n");
        assert!(parse_messages(&content, "session-limit").is_empty());
    }

    #[test]
    fn rejects_transcript_with_too_many_malformed_lines() {
        let mut lines =
            vec![r#"{"type":"session","id":"session-line-limit","version":3}"#.to_owned()];
        lines.resize(200_001, "not-json".to_owned());
        let content = lines.join("\n");
        assert!(parse_messages(&content, "session-line-limit").is_empty());
    }

    #[test]
    fn rejects_transcript_when_session_header_is_malformed() {
        let content = [
            r#"{"type":"session","id":"session-tail""#,
            r#"{"type":"session","id":"session-tail","version":3}"#,
            r#"{"type":"message","id":"m1","parentId":null,"message":{"role":"user","content":"must not be shown"}}"#,
        ]
        .join("\n");

        assert!(parse_messages(&content, "session-tail").is_empty());
    }

    #[test]
    fn rejects_transcript_with_mismatched_session_header() {
        let content = [
            r#"{"type":"session","id":"different-session","version":3}"#,
            r#"{"type":"message","id":"m1","parentId":null,"message":{"role":"user","content":"wrong conversation"}}"#,
        ]
        .join("\n");

        assert!(parse_messages(&content, "requested-session").is_empty());
    }

    #[test]
    fn keeps_active_messages_across_compaction_and_ignores_other_branches() {
        let content = [
            r#"{"type":"session","id":"session-1","version":3}"#,
            r#"{"type":"message","id":"u1","parentId":null,"message":{"role":"user","content":"before compact"}}"#,
            r#"{"type":"message","id":"a1","parentId":"u1","message":{"role":"assistant","content":"answer before compact"}}"#,
            r#"{"type":"message","id":"alt","parentId":"u1","message":{"role":"user","content":"other branch"}}"#,
            r#"{"type":"compaction","id":"c1","parentId":"a1","firstKeptEntryId":"a1","summary":"summary"}"#,
            r#"{"type":"message","id":"u2","parentId":"c1","message":{"role":"user","content":"after compact"}}"#,
        ]
        .join("\n");

        let messages = parse_messages(&content, "session-1");
        let contents = messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            contents,
            vec![
                "before compact",
                "answer before compact",
                "summary",
                "after compact"
            ]
        );
        assert_eq!(messages[2].role, "compaction");
    }

    #[test]
    fn rejects_tree_with_duplicate_entry_ids() {
        let content = [
            r#"{"type":"session","id":"session-duplicate","version":3}"#,
            r#"{"type":"message","id":"same","parentId":null,"message":{"role":"user","content":"first"}} "#,
            r#"{"type":"message","id":"same","parentId":null,"message":{"role":"assistant","content":"ambiguous duplicate"}}"#,
        ]
        .join("\n");

        assert!(parse_messages(&content, "session-duplicate").is_empty());
    }

    #[test]
    fn handles_missing_parent_and_cyclic_parent_links_without_panicking() {
        let missing_parent = [
            r#"{"type":"session","id":"session-2","version":3}"#,
            r#"{"type":"message","id":"u1","parentId":"deleted-entry","message":{"role":"user","content":"surviving leaf"}}"#,
        ]
        .join("\n");
        let messages = parse_messages(&missing_parent, "session-2");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "surviving leaf");

        let cycle = [
            r#"{"type":"session","id":"session-3","version":3}"#,
            r#"{"type":"message","id":"a","parentId":"b","message":{"role":"user","content":"A"}}"#,
            r#"{"type":"message","id":"b","parentId":"a","message":{"role":"assistant","content":"B"}}"#,
        ]
        .join("\n");
        let messages = parse_messages(&cycle, "session-3");
        assert_eq!(messages.len(), 2);
    }

    #[test]
    fn detects_when_native_session_file_is_deleted() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let workspace = std::env::temp_dir().join(format!(
            "pi-transcript-missing-{}-{unique}",
            std::process::id()
        ));
        let session_dir = workspace
            .join(".agentweb-state")
            .join("pi-sessions")
            .join("project");
        std::fs::create_dir_all(&session_dir).unwrap();
        let transcript = session_dir.join("2026-01-01T00-00-00-000Z_deleted-id.jsonl");
        std::fs::write(&transcript, r#"{"type":"session","id":"deleted-id"}"#).unwrap();

        assert_eq!(
            native_session_file_exists_from_workspace(&workspace, "deleted-id"),
            NativeSessionFileLookup::Found
        );
        std::fs::remove_file(&transcript).unwrap();
        assert_eq!(
            native_session_file_exists_from_workspace(&workspace, "deleted-id"),
            NativeSessionFileLookup::Missing
        );

        std::fs::remove_dir_all(workspace).ok();
    }

    #[test]
    fn rejects_symlinked_session_directory_for_existence_check() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let workspace = std::env::temp_dir().join(format!(
                "pi-transcript-symlink-{}-{unique}",
                std::process::id()
            ));
            let outside = std::env::temp_dir().join(format!(
                "pi-transcript-outside-{}-{unique}",
                std::process::id()
            ));
            std::fs::create_dir_all(workspace.join(".agentweb-state")).unwrap();
            std::fs::create_dir_all(&outside).unwrap();
            std::fs::write(
                outside.join("2026-01-01T00-00-00-000Z_escape-id.jsonl"),
                r#"{"type":"session","id":"escape-id"}"#,
            )
            .unwrap();
            symlink(
                &outside,
                workspace.join(".agentweb-state").join("pi-sessions"),
            )
            .unwrap();

            assert_eq!(
                native_session_file_exists_from_workspace(&workspace, "escape-id"),
                NativeSessionFileLookup::Unknown
            );

            std::fs::remove_dir_all(workspace).ok();
            std::fs::remove_dir_all(outside).ok();
        }
    }

    #[test]
    fn reads_user_assistant_and_tool_result_messages() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("pi-transcript-{}-{unique}", std::process::id()));
        let nested = dir
            .join(".agentweb-state")
            .join("pi-sessions")
            .join("--workspaces--");
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

        assert_eq!(messages.len(), 3);
        assert!(messages
            .iter()
            .all(|message| message.content != "abandoned branch"));
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "hi");
        assert_eq!(
            messages[0].created_at.as_deref(),
            Some("2026-01-01T00:00:01Z")
        );
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[1].content, "yo");
        // Thinking is preserved as a structured part, not dropped.
        let parts = messages[1].parts.as_ref().unwrap();
        assert_eq!(parts[0].kind, "thinking");
        assert_eq!(parts[0].text.as_deref(), Some("hmm"));
        assert_eq!(parts[1].kind, "text");
        assert_eq!(parts[1].text.as_deref(), Some("yo"));
        assert_eq!(messages[2].role, "toolResult");
        assert_eq!(messages[2].content, "out");
    }

    #[test]
    fn keeps_tool_call_parts_of_a_textless_assistant_turn() {
        let content = [
            r#"{"type":"session","id":"session-tools","version":3}"#,
            r#"{"type":"message","id":"a1","parentId":null,"message":{"role":"assistant","model":"gpt-test","stopReason":"toolUse","durationMs":1500,"content":[{"type":"thinking","thinking":"plan"},{"type":"toolCall","id":"call-1","name":"bash","arguments":{"command":"ls"}}],"usage":{"input":10,"output":5,"cacheRead":2,"cacheWrite":0,"totalTokens":17,"cost":{"total":0.01}}}}"#,
            r#"{"type":"message","id":"r1","parentId":"a1","message":{"role":"toolResult","toolCallId":"call-1","toolName":"bash","isError":true,"durationMs":42,"details":{"diff":"-old\n+new","patch":"--- a\n+++ b\n@@ -1 +1 @@\n-old\n+new","truncation":{"content":"tail"}},"content":[{"type":"text","text":"boom"}]}}"#,
        ]
        .join("\n");

        let messages = parse_messages(&content, "session-tools");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content, "");
        let parts = messages[0].parts.as_ref().unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[1].kind, "toolCall");
        assert_eq!(parts[1].name.as_deref(), Some("bash"));
        assert_eq!(parts[1].arguments.as_ref().unwrap()["command"], "ls");
        assert_eq!(messages[0].usage.as_ref().unwrap()["totalTokens"], 17);
        assert_eq!(messages[0].model.as_deref(), Some("gpt-test"));
        assert_eq!(messages[0].stop_reason.as_deref(), Some("toolUse"));
        assert_eq!(messages[0].duration_ms, Some(1500));
        assert!(messages[0].error_message.is_none());
        assert_eq!(messages[1].tool_call_id.as_deref(), Some("call-1"));
        assert_eq!(messages[1].is_error, Some(true));
        assert_eq!(messages[1].tool_name.as_deref(), Some("bash"));
        assert_eq!(messages[1].duration_ms, Some(42));
        assert_eq!(messages[1].truncated, Some(true));
        assert_eq!(messages[1].diff.as_deref(), Some("-old\n+new"));
        assert_eq!(
            messages[1].patch.as_deref(),
            Some("--- a\n+++ b\n@@ -1 +1 @@\n-old\n+new")
        );
    }

    #[test]
    fn keeps_an_errored_assistant_turn_with_no_content() {
        // Pi reports provider failures as an empty assistant turn plus an
        // `errorMessage`; dropping it would silently lose the turn.
        let content = [
            r#"{"type":"session","id":"session-error","version":3}"#,
            r#"{"type":"message","id":"a1","parentId":null,"message":{"role":"assistant","content":[],"stopReason":"error","errorMessage":"Request timed out."}}"#,
        ]
        .join("\n");

        let messages = parse_messages(&content, "session-error");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "assistant");
        assert_eq!(messages[0].content, "");
        assert_eq!(messages[0].stop_reason.as_deref(), Some("error"));
        assert_eq!(
            messages[0].error_message.as_deref(),
            Some("Request timed out.")
        );
        assert!(messages[0].diff.is_none());
        assert!(messages[0].patch.is_none());
        assert!(messages[0].tool_name.is_none());
    }

    #[test]
    fn decodes_base64_payloads() {
        assert_eq!(decode_base64("aGVsbG8="), Some(b"hello".to_vec()));
        assert_eq!(decode_base64(""), Some(Vec::new()));
        assert_eq!(decode_base64("aGVsbG8"), Some(b"hello".to_vec()));
        assert_eq!(decode_base64("!!!"), None);
    }

    #[test]
    fn exposes_tool_result_image_parts_without_inlining_bytes() {
        let content = [
            r#"{"type":"session","id":"session-img","version":3}"#,
            r#"{"type":"message","id":"a1","parentId":null,"message":{"role":"assistant","content":[{"type":"toolCall","id":"c1","name":"screenshot","arguments":{}}]}}"#,
            r#"{"type":"message","id":"r1","parentId":"a1","message":{"role":"toolResult","toolCallId":"c1","content":[{"type":"text","text":"shot"},{"type":"image","mimeType":"image/png","data":"aGVsbG8="}]}}"#,
        ]
        .join("\n");

        let messages = parse_messages(&content, "session-img");
        assert_eq!(messages[1].content, "shot");
        let parts = messages[1].parts.as_ref().unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].kind, "text");
        assert_eq!(parts[1].kind, "image");
        assert_eq!(parts[1].mime_type.as_deref(), Some("image/png"));
        // Index refers to the original content array, not the filtered parts.
        assert_eq!(parts[1].image_index, Some(1));
    }

    #[test]
    fn reads_image_bytes_by_entry_id_and_part_index() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pi-transcript-image-{unique}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("2026-01-01T00-00-00-000Z_img-id.jsonl"),
            [
                r#"{"type":"session","id":"img-id","version":3}"#,
                r#"{"type":"message","id":"r1","parentId":null,"message":{"role":"toolResult","toolCallId":"c1","content":[{"type":"image","mimeType":"image/png","data":"aGVsbG8="}]}}"#,
            ]
            .join("\n"),
        )
        .unwrap();

        let (mime, bytes) = read_entry_image_from_root(&dir, "img-id", "r1", 0).unwrap();
        assert_eq!(mime, "image/png");
        assert_eq!(bytes, b"hello");
        assert!(read_entry_image_from_root(&dir, "img-id", "r1", 1).is_none());
        assert!(read_entry_image_from_root(&dir, "img-id", "missing", 0).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn searches_transcripts_across_sessions_and_reports_snippets() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pi-transcript-search-{unique}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("2026-01-01T00-00-00-000Z_alpha.jsonl"),
            [
                r#"{"type":"session","id":"alpha","version":3}"#,
                r#"{"type":"message","id":"e1","message":{"role":"user","content":"please fix the flaky Retry test"}}"#,
                r#"{"type":"message","id":"e2","message":{"role":"assistant","content":[{"type":"text","text":"the retry loop was wrong"}]}}"#,
            ]
            .join("\n"),
        )
        .unwrap();
        // Same filename suffix, different header id: it must not answer for `beta`.
        std::fs::write(
            dir.join("2026-01-02T00-00-00-000Z_beta.jsonl"),
            [
                r#"{"type":"session","id":"other","version":3}"#,
                r#"{"type":"message","id":"b1","message":{"role":"user","content":"beta retry"}}"#,
            ]
            .join("\n"),
        )
        .unwrap();

        let ids = vec!["alpha".to_owned(), "missing".to_owned(), "beta".to_owned()];
        let (hits, truncated) =
            search_transcripts_in_root(&dir, &ids, "RETRY", std::time::Duration::from_secs(5));
        // Case-insensitive, one hit per session, and the untrusted header is skipped.
        assert_eq!(hits.len(), 1);
        assert!(!truncated);
        let hit = &hits[0];
        assert_eq!(hit.session_id, "alpha");
        assert_eq!(hit.entry_id, "e1");
        assert_eq!(hit.role, "user");
        assert_eq!(hit.before, "please fix the flaky");
        assert_eq!(hit.matched, "Retry");
        assert_eq!(hit.after, "test");

        let (none, _) = search_transcripts_in_root(
            &dir,
            &ids,
            "nothing here",
            std::time::Duration::from_secs(5),
        );
        assert!(none.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn finds_matches_on_character_boundaries() {
        // A distant multi-byte needle exercises both snippet windows.
        let text = format!("{}关键字{}", "前".repeat(80), "后".repeat(80));
        let (start, end) = find_ci(&text, "关键字").unwrap();
        let (before, matched, after) = snippet(&text, start, end);
        assert_eq!(matched, "关键字");
        assert_eq!(before.chars().filter(|c| *c == '前').count(), 60);
        assert!(before.starts_with('…'));
        assert!(before.ends_with('前'));
        assert!(after.ends_with('…'));
        assert!(find_ci(&text, "不存在").is_none());
        assert!(find_ci(&text, "").is_none());
    }

    #[test]
    fn reads_the_session_id_out_of_a_transcript_filename() {
        assert_eq!(
            session_id_from_name(Path::new("/p/2026-01-01T00-00-00-000Z_abc-123.jsonl")),
            Some("abc-123")
        );
        assert_eq!(session_id_from_name(Path::new("/p/notes.txt")), None);
        assert_eq!(session_id_from_name(Path::new("/p/abc.jsonl")), None);
    }
}
