//! Read chat messages from a pi native session transcript.
//!
//! pi stores each session as JSONL under `$PI_CODING_AGENT_DIR/sessions` (or
//! `$HOME/.pi/agent/sessions`), one directory per working directory. Every
//! `message` entry carries the provider message that agentweb renders.
use serde::Serialize;
use serde_json::Value;
use std::io::Read;
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
        return NativeSessionDiscovery { sessions: Vec::new(), complete: false };
    };
    let root_meta = match std::fs::symlink_metadata(&root) {
        Ok(metadata) if !metadata.file_type().is_symlink() && metadata.is_dir() => metadata,
        _ => return NativeSessionDiscovery { sessions: Vec::new(), complete: false },
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
            Err(_) => { complete = false; continue; }
        };
        for entry in entries {
            scanned += 1;
            if scanned > MAX_ENTRIES {
                complete = false;
                break;
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => { complete = false; continue; }
            };
            let path = entry.path();
            let metadata = match std::fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(_) => { complete = false; continue; }
            };
            if metadata.file_type().is_symlink() { continue; }
            if metadata.is_dir() {
                if depth < MAX_DEPTH { pending.push((path, depth + 1)); }
                else { complete = false; }
                continue;
            }
            if !metadata.is_file()
                || path.extension().and_then(|v| v.to_str()) != Some("jsonl")
            { continue; }
            let file = match std::fs::File::open(&path) {
                Ok(file) => file,
                Err(_) => { complete = false; continue; }
            };
            let mut reader = std::io::BufReader::new(file);
            let mut first_line = String::new();
            if std::io::BufRead::read_line(&mut reader, &mut first_line).is_err()
                || first_line.len() > MAX_HEADER_BYTES
            { complete = false; continue; }
            let Ok(header) = serde_json::from_str::<Value>(&first_line) else { continue };
            if header.get("type").and_then(Value::as_str) != Some("session") { continue; }
            let Some(id) = header.get("id").and_then(Value::as_str) else { continue };
            let Some(cwd) = header.get("cwd").and_then(Value::as_str) else { continue };
            if id.is_empty() || cwd.is_empty() { continue; }

            let mut title = String::new();
            let mut scanned_bytes = first_line.len();
            while scanned_bytes < MAX_TITLE_SCAN_BYTES {
                let mut line = String::new();
                let read = match std::io::BufRead::read_line(&mut reader, &mut line) {
                    Ok(read) => read,
                    Err(_) => { complete = false; break; }
                };
                if read == 0 { break; }
                scanned_bytes += read;
                let Ok(entry) = serde_json::from_str::<Value>(&line) else { continue };
                if entry.get("type").and_then(Value::as_str) != Some("message") { continue; }
                let Some(message) = entry.get("message") else { continue };
                if message.get("role").and_then(Value::as_str) != Some("user") { continue };
                if let Some(text) = message.get("content").and_then(text_content) {
                    title = text.split_whitespace().collect::<Vec<_>>().join(" ");
                    if !title.is_empty() { break; }
                }
            }
            if scanned_bytes >= MAX_TITLE_SCAN_BYTES {
                complete = false;
            }
            if title.is_empty() { title = format!("Pi session {}", &id.chars().take(8).collect::<String>()); }
            title = title.chars().take(80).collect();
            let timestamp = header.get("timestamp").and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| metadata.modified().ok()
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|duration| chrono::DateTime::<chrono::Utc>::from_timestamp(duration.as_secs() as i64, 0)
                        .map(|value| value.to_rfc3339()).unwrap_or_default())
                    .unwrap_or_default());
            let updated_at = metadata.modified().ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .and_then(|duration| chrono::DateTime::<chrono::Utc>::from_timestamp(duration.as_secs() as i64, 0))
                .map(|value| value.to_rfc3339())
                .unwrap_or_else(|| timestamp.clone());
            sessions.push(NativeSessionSummary {
                id: id.to_owned(), cwd: cwd.to_owned(), title,
                created_at: timestamp, updated_at,
            });
        }
        if scanned > MAX_ENTRIES { break; }
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
    match find_by_suffix(&root, &format!("_{native_session_id}.jsonl"), native_session_id) {
        Ok(Some(_)) => NativeSessionFileLookup::Found,
        Ok(None) => NativeSessionFileLookup::Missing,
        Err(()) => NativeSessionFileLookup::Unknown,
    }
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

/// Return whether a valid Pi transcript file exists in the workspace-scoped
/// session directory. This lets API callers distinguish a deleted native
/// session from a valid transcript that simply contains no visible messages.
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
    match find_by_suffix(&session_dir, &format!("_{native_session_id}.jsonl"), native_session_id) {
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
    // Keep a single transcript refresh from allocating unbounded memory.
    // Typical conversations are far smaller; oversized/corrupt files fail
    // closed instead of blocking the API worker with a huge JSONL parse.
    const MAX_TRANSCRIPT_BYTES: u64 = 64 * 1024 * 1024;
    // Open once, then inspect and read from that same handle. This avoids a
    // path-based metadata/read race where the candidate could be replaced
    // between the size check and the read.
    let Ok(file) = std::fs::File::open(&path) else {
        return Vec::new();
    };
    let Ok(metadata) = file.metadata() else {
        return Vec::new();
    };
    if !metadata.is_file() || metadata.len() > MAX_TRANSCRIPT_BYTES {
        return Vec::new();
    }
    // Bound the actual read too: the file may grow after metadata() returns.
    // A partial final UTF-8 sequence or JSON object should not hide prior
    // messages; lossy decoding lets the parser skip only the incomplete tail.
    let mut bytes = Vec::new();
    if file
        .take(MAX_TRANSCRIPT_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() as u64 > MAX_TRANSCRIPT_BYTES
    {
        return Vec::new();
    }
    let content = String::from_utf8_lossy(&bytes);
    parse_messages(&content, native_session_id)
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
                && path.file_name().and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with(suffix))
            {
                let file = match std::fs::File::open(&path) {
                    Ok(file) => file,
                    Err(_) => return Err(()),
                };
                const MAX_HEADER_BYTES: u64 = 64 * 1024;
                let mut reader = std::io::BufReader::new(file).take(MAX_HEADER_BYTES);
                let mut first_line = String::new();
                if std::io::BufRead::read_line(&mut reader, &mut first_line).is_err()
                    || first_line.len() as u64 >= MAX_HEADER_BYTES
                {
                    return Err(());
                }
                let Ok(header) = serde_json::from_str::<Value>(&first_line) else {
                    return Err(());
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

pub fn read_messages(native_session_id: &str) -> Vec<TranscriptMessage> {
    let Some(root) = sessions_root() else {
        return Vec::new();
    };
    read_messages_from_root(&root, native_session_id)
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

        let path = find_by_suffix(&dir, "_same-id.jsonl", "same-id").unwrap().unwrap();
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

        assert!(matches!(find_by_suffix(&dir, "_large-id.jsonl", "large-id"), Ok(None) | Err(())));
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
            vec!["before compact", "answer before compact", "after compact"]
        );
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

        assert_eq!(native_session_file_exists_from_workspace(
            &workspace,
            "deleted-id"
        ), NativeSessionFileLookup::Found);
        std::fs::remove_file(&transcript).unwrap();
        assert_eq!(native_session_file_exists_from_workspace(
            &workspace,
            "deleted-id"
        ), NativeSessionFileLookup::Missing);

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

            assert_eq!(native_session_file_exists_from_workspace(
                &workspace,
                "escape-id"
            ), NativeSessionFileLookup::Unknown);

            std::fs::remove_dir_all(workspace).ok();
            std::fs::remove_dir_all(outside).ok();
        }
    }

    #[test]
    fn reads_only_user_and_assistant_messages() {
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

        assert_eq!(messages.len(), 2);
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
    }
}
