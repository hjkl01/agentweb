//! Read OpenCode's native SQLite transcript without modifying its database.
//! Supports the legacy session/message/part schema and the current
//! session_v2/session_message schema. The database format is owned by OpenCode.
use super::pi_transcript::{TranscriptMessage, TranscriptPart};
use serde_json::Value;
use sqlx::{Connection, Row, sqlite::SqliteConnectOptions};
use std::{path::PathBuf, time::Duration};

const MAX_MESSAGES: usize = 20_000;
const MAX_PARTS_PER_MESSAGE: usize = 5_000;

fn database_path() -> PathBuf {
    if let Some(path) = std::env::var_os("OPENCODE_DB") {
        return PathBuf::from(path);
    }
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| super::config::agent_home().join(".local/share"));
    data_home.join("opencode/opencode.db")
}

fn safe_session_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn timestamp(value: Option<i64>) -> Option<String> {
    value.and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis)
        .map(|v| v.to_rfc3339())
}

fn empty_message(id: String, role: &str, created_at: Option<String>) -> TranscriptMessage {
    TranscriptMessage {
        id, role: role.to_owned(), content: String::new(), created_at,
        parts: None, tool_call_id: None, is_error: None, usage: None,
        tool_name: None, duration_ms: None, truncated: None, diff: None,
        patch: None, model: None, stop_reason: None, error_message: None,
    }
}

fn text_part(value: &str) -> TranscriptPart {
    TranscriptPart { kind: "text".into(), text: Some(value.to_owned()), ..Default::default() }
}

fn add_part(message: &mut TranscriptMessage, part: TranscriptPart) {
    if part.kind == "text" {
        if let Some(text) = part.text.as_deref() {
            message.content.push_str(text);
        }
    }
    message.parts.get_or_insert_with(Vec::new).push(part);
}

fn parse_v1_part(message: &mut TranscriptMessage, data: &Value) {
    match data.get("type").and_then(Value::as_str).unwrap_or("") {
        "text" => {
            if let Some(text) = data.get("text").or_else(|| data.get("content")).and_then(Value::as_str) {
                add_part(message, text_part(text));
            }
        }
        "reasoning" => {
            if let Some(text) = data.get("text").or_else(|| data.get("content")).and_then(Value::as_str) {
                add_part(message, TranscriptPart { kind: "thinking".into(), text: Some(text.to_owned()), ..Default::default() });
            }
        }
        "tool" => {
            let state = data.get("state").unwrap_or(&Value::Null);
            let status = state.get("status").and_then(Value::as_str).unwrap_or("");
            let call_id = data.get("callID").or_else(|| data.get("callId")).and_then(Value::as_str).map(str::to_owned);
            let name = data.get("tool").and_then(Value::as_str).map(str::to_owned);
            let args = state.get("input").cloned().or_else(|| data.get("input").cloned());
            add_part(message, TranscriptPart {
                kind: "toolCall".into(), name: name.clone(), arguments: args,
                tool_call_id: call_id.clone(), ..Default::default()
            });
            if matches!(status, "completed" | "error") {
                let output = state.get("output").and_then(Value::as_str).unwrap_or("").to_owned();
                let mut result = empty_message(
                    format!("{}-{}", message.id, call_id.as_deref().unwrap_or("tool")),
                    "tool", message.created_at.clone(),
                );
                result.content = output.clone();
                result.parts = (!output.is_empty()).then(|| vec![text_part(&output)]);
                result.tool_call_id = call_id;
                result.tool_name = name;
                result.is_error = Some(status == "error");
                // Tool result is emitted after its parent by the caller.
            }
        }
        _ => {}
    }
}

fn parse_v2_content(message: &mut TranscriptMessage, content: &Value) {
    let Some(items) = content.as_array() else { return; };
    for item in items {
        match item.get("type").and_then(Value::as_str).unwrap_or("") {
            "text" => if let Some(text) = item.get("text").and_then(Value::as_str) {
                add_part(message, text_part(text));
            },
            "reasoning" => if let Some(text) = item.get("text").or_else(|| item.get("reasoning")).and_then(Value::as_str) {
                add_part(message, TranscriptPart { kind: "thinking".into(), text: Some(text.to_owned()), ..Default::default() });
            },
            "tool" => {
                let state = item.get("state").unwrap_or(&Value::Null);
                let status = state.get("status").and_then(Value::as_str).unwrap_or("");
                let call_id = item.get("callID").and_then(Value::as_str).map(str::to_owned);
                let name = item.get("tool").and_then(Value::as_str).map(str::to_owned);
                add_part(message, TranscriptPart {
                    kind: "toolCall".into(), name: name.clone(),
                    arguments: state.get("input").cloned(), tool_call_id: call_id.clone(),
                    ..Default::default()
                });
                if matches!(status, "completed" | "error") {
                    let output = state.get("output").and_then(Value::as_str).unwrap_or("").to_owned();
                    let mut result = empty_message(
                        format!("{}-{}", message.id, call_id.as_deref().unwrap_or("tool")),
                        "tool", message.created_at.clone(),
                    );
                    result.content = output.clone();
                    result.parts = (!output.is_empty()).then(|| vec![text_part(&output)]);
                    result.tool_call_id = call_id;
                    result.tool_name = name;
                    result.is_error = Some(status == "error");
                    // Tool results are appended by parse_v2_message.
                    message.parts.get_or_insert_with(Vec::new).push(TranscriptPart {
                        kind: "toolResult".into(), text: Some(output),
                        name: result.tool_name, tool_call_id: result.tool_call_id,
                        ..Default::default()
                    });
                }
            }
            _ => {}
        }
    }
}

fn parse_v2_message(id: &str, kind: &str, data: &Value, created: Option<i64>) -> Option<TranscriptMessage> {
    let role = match kind {
        "user" => "user",
        "assistant" => "assistant",
        _ => return None,
    };
    let mut message = empty_message(id.to_owned(), role, timestamp(created));
    if role == "assistant" {
        message.model = data.get("modelID").and_then(Value::as_str).map(str::to_owned);
        message.usage = data.get("tokens").cloned();
        message.stop_reason = data.get("finish").and_then(Value::as_str).map(str::to_owned);
        message.error_message = data.get("error").and_then(Value::as_str).map(str::to_owned);
    }
    parse_v2_content(&mut message, data.get("content").unwrap_or(&Value::Null));
    if message.content.is_empty() && message.parts.as_ref().is_none_or(Vec::is_empty) {
        None
    } else {
        Some(message)
    }
}

async fn table_exists(conn: &mut sqlx::SqliteConnection, name: &str) -> Result<bool, sqlx::Error> {
    let found: Option<String> = sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type='table' AND name=?")
        .bind(name).fetch_optional(&mut *conn).await?;
    Ok(found.is_some())
}

/// Read one native OpenCode session. Missing databases/sessions return an empty transcript.
pub async fn read_messages(session_id: &str) -> Vec<TranscriptMessage> {
    if !safe_session_id(session_id) { return Vec::new(); }
    read_messages_from(&database_path(), session_id).await.unwrap_or_else(|error| {
        tracing::warn!(%error, "unable to read OpenCode transcript");
        Vec::new()
    })
}

async fn read_messages_from(path: &std::path::Path, session_id: &str) -> anyhow::Result<Vec<TranscriptMessage>> {
    if !path.is_file() { return Ok(Vec::new()); }
    let options = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))?
        .read_only(true).create_if_missing(false).busy_timeout(Duration::from_secs(2));
    let mut conn = sqlx::SqliteConnection::connect_with(&options).await?;

    // OpenCode 2.x stores typed message records in session_message.
    if table_exists(&mut conn, "session_message").await? {
        let rows = sqlx::query("SELECT id, type, data FROM session_message WHERE session_id=? ORDER BY time_created, id LIMIT ?")
            .bind(session_id).bind(MAX_MESSAGES as i64).fetch_all(&mut conn).await?;
        let mut messages = Vec::new();
        for row in rows {
            let id: String = row.try_get("id")?;
            let kind: String = row.try_get("type")?;
            let raw: String = row.try_get("data")?;
            let Ok(data) = serde_json::from_str::<Value>(&raw) else { continue; };
            let role = data.get("role").and_then(Value::as_str).unwrap_or(&kind);
            let created = data.pointer("/time/created").and_then(Value::as_i64);
            if let Some(message) = parse_v2_message(&id, role, &data, created) {
                messages.push(message);
            }
        }
        return Ok(messages);
    }

    // OpenCode 1.x / compatibility schema stores message metadata and parts separately.
    if !table_exists(&mut conn, "message").await? || !table_exists(&mut conn, "part").await? {
        return Ok(Vec::new());
    }
    let rows = sqlx::query("SELECT id, time_created, data FROM message WHERE session_id=? ORDER BY time_created, id LIMIT ?")
        .bind(session_id).bind(MAX_MESSAGES as i64).fetch_all(&mut conn).await?;
    let mut messages = Vec::new();
    for row in rows {
        let id: String = row.try_get("id")?;
        let created: Option<i64> = row.try_get("time_created").ok();
        let raw: String = row.try_get("data")?;
        let Ok(data) = serde_json::from_str::<Value>(&raw) else { continue; };
        let role = data.get("role").and_then(Value::as_str).unwrap_or("");
        if !matches!(role, "user" | "assistant") { continue; }
        let mut message = empty_message(id.clone(), role, timestamp(created.or_else(|| data.pointer("/time/created").and_then(Value::as_i64))));
        if role == "assistant" {
            message.model = data.get("modelID").and_then(Value::as_str).map(str::to_owned);
            message.usage = data.get("tokens").cloned();
            message.stop_reason = data.get("finish").and_then(Value::as_str).map(str::to_owned);
        }
        let parts = sqlx::query("SELECT data FROM part WHERE session_id=? AND message_id=? ORDER BY time_created, id LIMIT ?")
            .bind(session_id).bind(&id).bind(MAX_PARTS_PER_MESSAGE as i64).fetch_all(&mut conn).await?;
        for part in parts {
            let raw: String = part.try_get("data")?;
            if let Ok(value) = serde_json::from_str::<Value>(&raw) {
                parse_v1_part(&mut message, &value);
            }
        }
        if !message.content.is_empty() || message.parts.as_ref().is_some_and(|p| !p.is_empty()) {
            messages.push(message);
        }
    }
    Ok(messages)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_current_v2_user_and_assistant_records() {
        let data: Value = serde_json::json!({
            "role":"assistant", "modelID":"claude-sonnet",
            "tokens":{"input":12,"output":5}, "finish":"stop",
            "time":{"created":1793700000000},
            "content":[
                {"type":"text","text":"working"},
                {"type":"reasoning","text":"plan"},
                {"type":"tool","callID":"call-1","tool":"bash",
                 "state":{"status":"completed","input":{"command":"pwd"},"output":"/workspace"}}
            ]
        });
        let message = parse_v2_message("msg-1", "assistant", &data, None).unwrap();
        assert_eq!(message.content, "working");
        assert_eq!(message.model.as_deref(), Some("claude-sonnet"));
        let parts = message.parts.unwrap();
        assert_eq!(parts[1].kind, "thinking");
        assert_eq!(parts[2].kind, "toolCall");
        assert_eq!(parts[2].name.as_deref(), Some("bash"));
        assert_eq!(parts[3].kind, "toolResult");
    }

    #[test]
    fn rejects_unsafe_session_ids() {
        assert!(!safe_session_id("../escape"));
        assert!(!safe_session_id("nested/session"));
        assert!(safe_session_id("ses_123-abc"));
    }
}
