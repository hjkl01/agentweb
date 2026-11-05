//! Read OpenCode's native SQLite transcript in read-only mode.
//!
//! OpenCode owns this database. Agent Web never writes to it and never copies
//! transcript content into its own SQLite database.
use super::pi_transcript::{TranscriptMessage, TranscriptPart};
use serde_json::Value;
use sqlx::{sqlite::SqliteConnectOptions, Connection, Row};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

const MAX_MESSAGES: usize = 20_000;
const MAX_PARTS_PER_MESSAGE: usize = 5_000;

fn database_path() -> PathBuf {
    let home = super::config::agent_home();
    let data_dir = if let Some(path) = std::env::var_os("XDG_DATA_HOME") {
        PathBuf::from(path).join("opencode")
    } else if cfg!(target_os = "macos") {
        home.join("Library/Application Support/opencode")
    } else if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData/Roaming"))
            .join("opencode")
    } else {
        home.join(".local/share/opencode")
    };

    if let Some(path) = std::env::var_os("OPENCODE_DB") {
        let path = PathBuf::from(path);
        return if path.is_absolute() {
            path
        } else {
            data_dir.join(path)
        };
    }
    data_dir.join("opencode.db")
}

fn safe_session_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

const MAX_IMAGE_BYTES: usize = 12 * 1024 * 1024;

fn allowed_image_mime(mime: &str) -> bool {
    matches!(
        mime,
        "image/png" | "image/jpeg" | "image/gif" | "image/webp" | "image/bmp" | "image/avif"
    )
}

fn attachment_mime(value: &Value) -> Option<String> {
    let mime = value
        .get("mime")
        .or_else(|| value.get("mimeType"))
        .or_else(|| value.get("mime_type"))
        .and_then(Value::as_str)
        .or_else(|| {
            value.get("url").and_then(Value::as_str)
                .and_then(|url| url.strip_prefix("data:"))
                .and_then(|url| url.split_once(','))
                .and_then(|(metadata, _)| metadata.split(';').next())
        })?;
    allowed_image_mime(mime).then(|| mime.to_owned())
}

fn has_embedded_image(value: &Value) -> bool {
    if attachment_mime(value).is_none() {
        return false;
    }
    let url = value.get("url").and_then(Value::as_str).unwrap_or_default();
    let data_url = url
        .strip_prefix("data:")
        .and_then(|value| value.split_once(','))
        .is_some_and(|(metadata, _)| metadata.split(';').any(|part| part == "base64"));
    let encoded_data = value.get("encoding").and_then(Value::as_str) == Some("base64")
        && value
            .get("data")
            .or_else(|| value.get("content"))
            .and_then(Value::as_str)
            .is_some_and(|data| data.len() <= MAX_IMAGE_BYTES.saturating_mul(4) / 3 + 8);
    data_url || encoded_data
}

fn attachment_data(value: &Value) -> Option<(String, Vec<u8>)> {
    let mime = attachment_mime(value)?;
    let data = value
        .get("url")
        .or_else(|| value.get("data"))
        .or_else(|| value.get("content"))
        .and_then(Value::as_str)?;
    let encoded = if let Some((metadata, payload)) =
        data.strip_prefix("data:").and_then(|v| v.split_once(','))
    {
        if !metadata.split(';').any(|part| part == "base64") || metadata.split(';').next()? != mime
        {
            return None;
        }
        payload
    } else {
        // Only accept explicitly marked base64 payloads. File paths and remote
        // URLs are intentionally not followed by the server.
        if value.get("encoding").and_then(Value::as_str) != Some("base64") {
            return None;
        }
        data
    };
    let bytes = decode_base64(encoded)?;
    Some((mime, bytes))
}

fn decode_base64(input: &str) -> Option<Vec<u8>> {
    if input.len() > MAX_IMAGE_BYTES.saturating_mul(4) / 3 + 8 {
        return None;
    }
    let mut output = Vec::with_capacity(input.len() / 4 * 3);
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for byte in input.bytes() {
        if byte == b'=' || byte.is_ascii_whitespace() {
            continue;
        }
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
            if output.len() > MAX_IMAGE_BYTES {
                return None;
            }
        }
    }
    Some(output)
}

fn timestamp(value: Option<i64>) -> Option<String> {
    value
        .and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis)
        .map(|v| v.to_rfc3339())
}

fn message(id: String, role: &str, created_at: Option<String>) -> TranscriptMessage {
    TranscriptMessage {
        id,
        role: role.to_owned(),
        content: String::new(),
        created_at,
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
    }
}

fn text_part(value: &str) -> TranscriptPart {
    TranscriptPart {
        kind: "text".into(),
        text: Some(value.to_owned()),
        ..Default::default()
    }
}

fn add_part(target: &mut TranscriptMessage, part: TranscriptPart) {
    if part.kind == "text" {
        if let Some(text) = part.text.as_deref() {
            target.content.push_str(text);
        }
    }
    target.parts.get_or_insert_with(Vec::new).push(part);
}

fn output_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    }
}

fn tool_result(
    parent_id: &str,
    call_id: Option<String>,
    name: Option<String>,
    state: &Value,
    created_at: Option<String>,
) -> Option<TranscriptMessage> {
    let status = state.get("status").and_then(Value::as_str)?;
    if !matches!(status, "completed" | "error") {
        return None;
    }
    let output = if let Some(content) = state.get("content").and_then(Value::as_array) {
        content
            .iter()
            .map(|item| {
                item.get("text")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        item.get("type")
                            .and_then(Value::as_str)
                            .map(|kind| format!("[{kind}]"))
                            .unwrap_or_default()
                    })
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        output_text(state.get("output").unwrap_or(&Value::Null))
    };
    let mut result = message(
        format!("{}-{}", parent_id, call_id.as_deref().unwrap_or("tool")),
        "toolResult",
        created_at,
    );
    result.content = output.clone();
    result.parts = (!output.is_empty()).then(|| vec![text_part(&output)]);
    result.tool_call_id = call_id;
    result.tool_name = name;
    result.is_error = Some(status == "error");
    if status == "error" {
        result.error_message = state
            .get("error")
            .and_then(|error| error.get("message").or(Some(error)))
            .map(output_text);
    }
    Some(result)
}

/// Parse one OpenCode part and return a separate result message when a tool
/// has completed. Tool results use the same role/fields as Pi's transcript API.
fn parse_part(
    parent: &mut TranscriptMessage,
    data: &Value,
    part_index: usize,
    tool_result_message: &mut Vec<TranscriptMessage>,
) {
    match data.get("type").and_then(Value::as_str).unwrap_or("") {
        "text" => {
            if let Some(text) = data
                .get("text")
                .or_else(|| data.get("content"))
                .and_then(Value::as_str)
            {
                add_part(parent, text_part(text));
            }
        }
        "reasoning" => {
            if let Some(text) = data
                .get("text")
                .or_else(|| data.get("content"))
                .and_then(Value::as_str)
            {
                add_part(
                    parent,
                    TranscriptPart {
                        kind: "thinking".into(),
                        text: Some(text.to_owned()),
                        ..Default::default()
                    },
                );
            }
        }
        "tool" => {
            let state = data.get("state").unwrap_or(&Value::Null);
            let call_id = data
                .get("id")
                .or_else(|| data.get("callID"))
                .or_else(|| data.get("callId"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            let name = data
                .get("name")
                .or_else(|| data.get("tool"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            let arguments = state
                .get("input")
                .cloned()
                .or_else(|| data.get("input").cloned());
            add_part(
                parent,
                TranscriptPart {
                    kind: "toolCall".into(),
                    name: name.clone(),
                    arguments,
                    tool_call_id: call_id.clone(),
                    ..Default::default()
                },
            );
            if let Some(result) =
                tool_result(&parent.id, call_id, name, state, parent.created_at.clone())
            {
                tool_result_message.push(result);
            }
        }
        "file" | "image" => {
            if has_embedded_image(data) {
                let mime = attachment_mime(data).unwrap();
                add_part(
                    parent,
                    TranscriptPart {
                        kind: "image".into(),
                        mime_type: Some(mime),
                        image_index: Some(part_index),
                        ..Default::default()
                    },
                );
            } else {
                let label = data
                    .get("filename")
                    .or_else(|| data.get("url"))
                    .and_then(Value::as_str)
                    .unwrap_or("attachment");
                add_part(parent, text_part(&format!("[附件: {label}]")));
            }
        }
        _ => {}
    }
}

fn parse_v2_message(
    id: &str,
    kind: &str,
    data: &Value,
    created: Option<i64>,
) -> Vec<TranscriptMessage> {
    let role = data.get("role").and_then(Value::as_str).unwrap_or(kind);
    if !matches!(role, "user" | "assistant") {
        return Vec::new();
    }
    let created_at =
        timestamp(created.or_else(|| data.pointer("/time/created").and_then(Value::as_i64)));
    let mut parent = message(id.to_owned(), role, created_at);
    if role == "user" {
        if let Some(text) = data.get("text").and_then(Value::as_str) {
            add_part(&mut parent, text_part(text));
        }
        // OpenCode user messages may contain file attachments in addition to
        // text. Keep a safe display placeholder; never copy base64/blob data
        // into the transcript response.
        if let Some(files) = data.get("files").and_then(Value::as_array) {
            for (index, file) in files.iter().enumerate() {
                if has_embedded_image(file) {
                    let mime = attachment_mime(file).unwrap();
                    add_part(
                        &mut parent,
                        TranscriptPart {
                            kind: "image".into(),
                            mime_type: Some(mime),
                            image_index: Some(index),
                            ..Default::default()
                        },
                    );
                } else {
                    let label = file
                        .get("name")
                        .and_then(Value::as_str)
                        .or_else(|| file.get("mime").and_then(Value::as_str))
                        .unwrap_or("attachment");
                    add_part(&mut parent, text_part(&format!("[附件: {label}]")));
                }
            }
        }
    }
    if role == "assistant" {
        parent.model = data
            .get("modelID")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| {
                let model = data.get("model")?;
                let provider = model.get("providerID").and_then(Value::as_str)?;
                let id = model.get("id").and_then(Value::as_str)?;
                Some(format!("{provider}/{id}"))
            });
        parent.usage = data.get("tokens").cloned();
        parent.stop_reason = data
            .get("finish")
            .and_then(Value::as_str)
            .map(str::to_owned);
        parent.error_message = data
            .pointer("/error/message")
            .and_then(Value::as_str)
            .or_else(|| data.get("error").and_then(Value::as_str))
            .map(str::to_owned);
    }
    let mut results = Vec::new();
    if let Some(parts) = data.get("content").and_then(Value::as_array) {
        for (index, part) in parts.iter().enumerate() {
            parse_part(&mut parent, part, index, &mut results);
        }
    }
    // Preserve empty assistant turns when OpenCode recorded a provider
    // error; otherwise the most useful diagnostic silently disappears.
    let keep_parent = !parent.content.is_empty()
        || parent.parts.as_ref().is_some_and(|p| !p.is_empty())
        || parent.error_message.is_some();
    if keep_parent {
        let mut messages = vec![parent];
        messages.extend(results);
        messages
    } else {
        results
    }
}

async fn table_exists(conn: &mut sqlx::SqliteConnection, name: &str) -> Result<bool, sqlx::Error> {
    let found: Option<String> =
        sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type='table' AND name=?")
            .bind(name)
            .fetch_optional(&mut *conn)
            .await?;
    Ok(found.is_some())
}

/// Read one native OpenCode session. Missing databases/sessions return an empty transcript.
pub async fn read_messages(session_id: &str) -> Vec<TranscriptMessage> {
    if !safe_session_id(session_id) {
        return Vec::new();
    }
    match read_messages_from(&database_path(), session_id).await {
        Ok(messages) => messages,
        Err(error) => {
            tracing::warn!(%error, "unable to read OpenCode transcript");
            Vec::new()
        }
    }
}

/// Read an embedded OpenCode image attachment from its native read-only database.
pub async fn read_image(
    session_id: &str,
    entry_id: &str,
    index: usize,
) -> Option<(String, Vec<u8>)> {
    if !safe_session_id(session_id) || entry_id.is_empty() || entry_id.len() > 256 {
        return None;
    }
    match read_image_from(&database_path(), session_id, entry_id, index).await {
        Ok(image) => image,
        Err(error) => {
            tracing::warn!(%error, "unable to read OpenCode image attachment");
            None
        }
    }
}

async fn read_image_from(
    path: &Path,
    session_id: &str,
    entry_id: &str,
    index: usize,
) -> anyhow::Result<Option<(String, Vec<u8>)>> {
    if !path.is_file() {
        return Ok(None);
    }
    let options = SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .create_if_missing(false)
        .busy_timeout(Duration::from_secs(2));
    let mut conn = sqlx::SqliteConnection::connect_with(&options).await?;

    if table_exists(&mut conn, "session_message").await? {
        let raw: Option<String> = sqlx::query_scalar(
            "SELECT data FROM session_message WHERE session_id=? AND id=? LIMIT 1",
        )
        .bind(session_id)
        .bind(entry_id)
        .fetch_optional(&mut conn)
        .await?;
        if let Some(raw) = raw {
            if let Ok(data) = serde_json::from_str::<Value>(&raw) {
                if let Some(file) = data
                    .get("files")
                    .and_then(Value::as_array)
                    .and_then(|v| v.get(index))
                {
                    if let Some(image) = attachment_data(file) {
                        return Ok(Some(image));
                    }
                }
            }
        }
        if table_exists(&mut conn, "session_part").await? {
            let raw: Option<String> = sqlx::query_scalar(
                "SELECT data FROM session_part WHERE session_id=? AND message_id=? ORDER BY ordinal LIMIT 1 OFFSET ?",
            )
            .bind(session_id)
            .bind(entry_id)
            .bind(index as i64)
            .fetch_optional(&mut conn)
            .await?;
            if let Some(raw) = raw {
                if let Ok(data) = serde_json::from_str::<Value>(&raw) {
                    return Ok(attachment_data(&data));
                }
            }
        }
        return Ok(None);
    }

    if !table_exists(&mut conn, "message").await? {
        return Ok(None);
    }
    let raw: Option<String> =
        sqlx::query_scalar("SELECT data FROM message WHERE session_id=? AND id=? LIMIT 1")
            .bind(session_id)
            .bind(entry_id)
            .fetch_optional(&mut conn)
            .await?;
    if let Some(raw) = raw {
        if let Ok(data) = serde_json::from_str::<Value>(&raw) {
            if let Some(file) = data
                .get("files")
                .and_then(Value::as_array)
                .and_then(|v| v.get(index))
            {
                if let Some(image) = attachment_data(file) {
                    return Ok(Some(image));
                }
            }
        }
    }
    if table_exists(&mut conn, "part").await? {
        let raw: Option<String> = sqlx::query_scalar(
            "SELECT data FROM part WHERE session_id=? AND message_id=? ORDER BY time_created, id LIMIT 1 OFFSET ?",
        )
        .bind(session_id)
        .bind(entry_id)
        .bind(index as i64)
        .fetch_optional(&mut conn)
        .await?;
        if let Some(raw) = raw {
            if let Ok(data) = serde_json::from_str::<Value>(&raw) {
                return Ok(attachment_data(&data));
            }
        }
    }
    Ok(None)
}

async fn read_messages_from(
    path: &Path,
    session_id: &str,
) -> anyhow::Result<Vec<TranscriptMessage>> {
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let options = SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .create_if_missing(false)
        .busy_timeout(Duration::from_secs(2));
    let mut conn = sqlx::SqliteConnection::connect_with(&options).await?;

    // Current OpenCode stores message envelopes in session_message and,
    // in newer releases, assistant content in the ordered session_part table.
    if table_exists(&mut conn, "session_message").await? {
        let rows = sqlx::query(
            "SELECT id, type, time_created, data FROM session_message WHERE session_id=? ORDER BY seq, id LIMIT ?",
        )
        .bind(session_id)
        .bind(MAX_MESSAGES as i64)
        .fetch_all(&mut conn)
        .await?;
        let has_parts = table_exists(&mut conn, "session_part").await?;
        let mut messages = Vec::new();
        for row in rows {
            let id: String = row.try_get("id")?;
            let kind: String = row.try_get("type")?;
            let created: Option<i64> = row.try_get("time_created").ok();
            let raw: String = row.try_get("data")?;
            let Ok(mut data) = serde_json::from_str::<Value>(&raw) else {
                continue;
            };
            // Recent OpenCode releases move assistant content into ordered
            // session_part rows. Match OpenCode's history.withParts behavior:
            // when parts exist, they replace the inline content envelope.
            if has_parts && kind == "assistant" {
                let parts = sqlx::query(
                    "SELECT data FROM session_part WHERE session_id=? AND message_id=? ORDER BY ordinal LIMIT ?",
                )
                .bind(session_id)
                .bind(&id)
                .bind(MAX_PARTS_PER_MESSAGE as i64)
                .fetch_all(&mut conn)
                .await?;
                if !parts.is_empty() {
                    let mut content = Vec::with_capacity(parts.len());
                    for part in parts {
                        let raw: String = part.try_get("data")?;
                        if let Ok(value) = serde_json::from_str::<Value>(&raw) {
                            content.push(value);
                        }
                    }
                    if let Some(object) = data.as_object_mut() {
                        object.insert("content".into(), Value::Array(content));
                    }
                }
            }
            messages.extend(parse_v2_message(&id, &kind, &data, created));
        }
        return Ok(messages);
    }

    // OpenCode's message/part schema stores message metadata and parts separately.
    if !table_exists(&mut conn, "message").await? || !table_exists(&mut conn, "part").await? {
        return Ok(Vec::new());
    }
    let rows = sqlx::query(
        "SELECT id, time_created, data FROM message WHERE session_id=? ORDER BY time_created, id LIMIT ?",
    ).bind(session_id).bind(MAX_MESSAGES as i64).fetch_all(&mut conn).await?;
    let mut messages = Vec::new();
    for row in rows {
        let id: String = row.try_get("id")?;
        let created: Option<i64> = row.try_get("time_created").ok();
        let raw: String = row.try_get("data")?;
        let Ok(data) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        let role = data.get("role").and_then(Value::as_str).unwrap_or("");
        if !matches!(role, "user" | "assistant") {
            continue;
        }
        let created_at =
            timestamp(created.or_else(|| data.pointer("/time/created").and_then(Value::as_i64)));
        let mut parent = message(id.clone(), role, created_at);
        if role == "assistant" {
            parent.model = data
                .get("modelID")
                .and_then(Value::as_str)
                .map(str::to_owned);
            parent.usage = data.get("tokens").cloned();
            parent.stop_reason = data
                .get("finish")
                .and_then(Value::as_str)
                .map(str::to_owned);
            parent.error_message = data
                .pointer("/error/message")
                .and_then(Value::as_str)
                .or_else(|| data.get("error").and_then(Value::as_str))
                .map(str::to_owned);
        }
        let parts = sqlx::query(
            "SELECT data FROM part WHERE session_id=? AND message_id=? ORDER BY time_created, id LIMIT ?",
        ).bind(session_id).bind(&id).bind(MAX_PARTS_PER_MESSAGE as i64).fetch_all(&mut conn).await?;
        let mut results = Vec::new();
        for (index, part) in parts.into_iter().enumerate() {
            let raw: String = part.try_get("data")?;
            if let Ok(value) = serde_json::from_str::<Value>(&raw) {
                parse_part(&mut parent, &value, index, &mut results);
            }
        }
        if !parent.content.is_empty() || parent.parts.as_ref().is_some_and(|p| !p.is_empty()) {
            messages.push(parent);
            messages.extend(results);
        }
    }
    Ok(messages)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_embedded_image_metadata_without_returning_base64() {
        let data = serde_json::json!({
            "role": "user",
            "files": [{
                "name": "screen.png",
                "mime": "image/png",
                "url": "data:image/png;base64,c2VjcmV0"
            }]
        });
        let messages = parse_v2_message("user-image", "user", &data, None);
        let parts = messages[0].parts.as_ref().unwrap();
        assert_eq!(parts[0].kind, "image");
        assert_eq!(parts[0].mime_type.as_deref(), Some("image/png"));
        assert_eq!(parts[0].image_index, Some(0));
        let serialized = serde_json::to_string(&messages).unwrap();
        assert!(!serialized.contains("c2VjcmV0"));
    }

    #[test]
    fn parses_text_thinking_and_tool_result_as_separate_messages() {
        let data: Value = serde_json::json!({
            "role":"assistant",
            "modelID":"claude-sonnet",
            "tokens":{"input":12,"output":5},
            "finish":"tool-calls",
            "time":{"created":1793700000000_i64},
            "content":[
                {"type":"text","text":"working"},
                {"type":"reasoning","text":"plan"},
                {"type":"tool","callID":"call-1","tool":"bash",
                 "state":{"status":"completed","input":{"command":"pwd"},"output":"/workspace"}}
            ]
        });
        let messages = parse_v2_message("msg-1", "assistant", &data, None);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "assistant");
        assert_eq!(messages[0].content, "working");
        assert_eq!(messages[0].model.as_deref(), Some("claude-sonnet"));
        let parts = messages[0].parts.as_ref().unwrap();
        assert_eq!(parts[1].kind, "thinking");
        assert_eq!(parts[2].kind, "toolCall");
        assert_eq!(parts[2].name.as_deref(), Some("bash"));
        assert_eq!(messages[1].role, "toolResult");
        assert_eq!(messages[1].content, "/workspace");
        assert_eq!(messages[1].tool_call_id.as_deref(), Some("call-1"));
    }

    #[test]
    fn keeps_empty_assistant_turn_with_provider_error() {
        let data = serde_json::json!({
            "role": "assistant",
            "error": {
                "type": "ProviderAuthError",
                "message": "Authentication expired"
            },
            "content": [],
            "time": {"created": 1793700000000_i64}
        });
        let messages = parse_v2_message("msg-error", "assistant", &data, None);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "assistant");
        assert_eq!(
            messages[0].error_message.as_deref(),
            Some("Authentication expired")
        );
    }

    #[test]
    fn preserves_user_file_attachments_without_exposing_file_data() {
        let data = serde_json::json!({
            "role": "user",
            "text": "",
            "files": [{
                "name": "design.png",
                "mime": "image/png",
                "data": "BASE64_SECRET_MUST_NOT_LEAK",
                "source": {"type": "inline"}
            }]
        });
        let messages = parse_v2_message("msg-file", "user", &data, None);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "[附件: design.png]");
        assert!(!messages[0].content.contains("BASE64_SECRET"));
    }

    #[test]
    fn retains_running_tool_call_without_fabricating_result() {
        let mut parent = message("m1".into(), "assistant", None);
        let mut results = Vec::new();
        parse_part(
            &mut parent,
            &serde_json::json!({
                "type":"tool","callID":"c1","tool":"bash",
                "state":{"status":"running","input":{"command":"pwd"}}
            }),
            0,
            &mut results,
        );
        assert_eq!(parent.parts.as_ref().unwrap()[0].kind, "toolCall");
        assert!(results.is_empty());
    }

    #[test]
    fn rejects_unsafe_session_ids() {
        assert!(!safe_session_id("../escape"));
        assert!(!safe_session_id("nested/session"));
        assert!(safe_session_id("ses_123-abc"));
    }

    async fn temporary_database(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "agentweb-opencode-{name}-{}.db",
            uuid::Uuid::new_v4()
        ))
    }

    #[tokio::test]
    async fn reads_legacy_message_part_database_fixture() {
        let path = temporary_database("legacy").await;
        let options = SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true);
        let mut conn = sqlx::SqliteConnection::connect_with(&options)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE message (id TEXT, session_id TEXT, time_created INTEGER, data TEXT)",
        )
        .execute(&mut conn)
        .await
        .unwrap();
        sqlx::query("CREATE TABLE part (id TEXT, session_id TEXT, message_id TEXT, time_created INTEGER, data TEXT)")
            .execute(&mut conn).await.unwrap();
        sqlx::query("INSERT INTO message VALUES (?, ?, ?, ?)")
            .bind("msg-user")
            .bind("ses_fixture")
            .bind(1793700000000_i64)
            .bind(r#"{"role":"user"}"#)
            .execute(&mut conn)
            .await
            .unwrap();
        sqlx::query("INSERT INTO part VALUES (?, ?, ?, ?, ?)")
            .bind("prt-user")
            .bind("ses_fixture")
            .bind("msg-user")
            .bind(1793700000001_i64)
            .bind(r#"{"type":"text","text":"hello from fixture"}"#)
            .execute(&mut conn)
            .await
            .unwrap();
        sqlx::query("INSERT INTO message VALUES (?, ?, ?, ?)")
            .bind("msg-assistant")
            .bind("ses_fixture")
            .bind(1793700000002_i64)
            .bind(r#"{"role":"assistant","modelID":"test-model","tokens":{"input":3,"output":2}}"#)
            .execute(&mut conn)
            .await
            .unwrap();
        sqlx::query("INSERT INTO part VALUES (?, ?, ?, ?, ?)")
            .bind("prt-tool").bind("ses_fixture").bind("msg-assistant").bind(1793700000003_i64)
            .bind(r#"{"type":"tool","callID":"call-fixture","tool":"bash","state":{"status":"completed","input":{"command":"pwd"},"output":"/tmp"}}"#)
            .execute(&mut conn).await.unwrap();
        conn.close().await.unwrap();

        let messages = read_messages_from(&path, "ses_fixture").await.unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].content, "hello from fixture");
        assert_eq!(messages[1].model.as_deref(), Some("test-model"));
        assert_eq!(messages[2].role, "toolResult");
        assert_eq!(messages[2].content, "/tmp");
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn reads_typed_session_message_database_fixture() {
        let path = temporary_database("typed").await;
        let options = SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true);
        let mut conn = sqlx::SqliteConnection::connect_with(&options)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE session_message (id TEXT, session_id TEXT, type TEXT, seq INTEGER, time_created INTEGER, time_updated INTEGER, data TEXT)")
            .execute(&mut conn).await.unwrap();
        sqlx::query("CREATE TABLE session_part (message_id TEXT, session_id TEXT, ordinal INTEGER, type TEXT, call_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT)")
            .execute(&mut conn).await.unwrap();

        sqlx::query("INSERT INTO session_message VALUES (?, ?, ?, ?, ?, ?, ?)")
            .bind("msg-user")
            .bind("ses_v2")
            .bind("user")
            .bind(1_i64)
            .bind(1793700000000_i64)
            .bind(1793700000000_i64)
            .bind(r#"{"text":"hello from OpenCode","time":{"created":1793700000000}}"#)
            .execute(&mut conn)
            .await
            .unwrap();
        sqlx::query("INSERT INTO session_message VALUES (?, ?, ?, ?, ?, ?, ?)")
            .bind("msg-v2").bind("ses_v2").bind("assistant").bind(2_i64)
            .bind(1793700001000_i64).bind(1793700001000_i64)
            .bind(r#"{"model":{"providerID":"anthropic","id":"claude-sonnet-4-5"},"tokens":{"input":12,"output":5},"finish":"tool-calls","content":[],"time":{"created":1793700001000}}"#)
            .execute(&mut conn).await.unwrap();

        for (ordinal, kind, call_id, data) in [
            (
                0_i64,
                "text",
                None,
                r#"{"type":"text","text":"v2 fixture"}"#,
            ),
            (
                1_i64,
                "reasoning",
                None,
                r#"{"type":"reasoning","text":"thinking"}"#,
            ),
            (
                2_i64,
                "tool",
                Some("call-v2"),
                r#"{"type":"tool","id":"call-v2","name":"read","state":{"status":"error","input":{"file":"x"},"content":[{"type":"text","text":"not found"}],"error":{"message":"read failed"}},"time":{"created":1793700001001}}"#,
            ),
        ] {
            sqlx::query("INSERT INTO session_part VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
                .bind("msg-v2")
                .bind("ses_v2")
                .bind(ordinal)
                .bind(kind)
                .bind(call_id)
                .bind(1793700001001_i64)
                .bind(1793700001001_i64)
                .bind(data)
                .execute(&mut conn)
                .await
                .unwrap();
        }
        conn.close().await.unwrap();

        let messages = read_messages_from(&path, "ses_v2").await.unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "hello from OpenCode");
        assert_eq!(messages[1].content, "v2 fixture");
        assert_eq!(
            messages[1].model.as_deref(),
            Some("anthropic/claude-sonnet-4-5")
        );
        assert_eq!(messages[1].parts.as_ref().unwrap()[1].kind, "thinking");
        assert_eq!(messages[2].role, "toolResult");
        assert_eq!(messages[2].content, "not found");
        assert_eq!(messages[2].is_error, Some(true));
        assert_eq!(messages[2].error_message.as_deref(), Some("read failed"));
        let _ = std::fs::remove_file(path);
    }
}
