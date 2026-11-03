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
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
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
            if let Some(text) = data.get("text").or_else(|| data.get("content")).and_then(Value::as_str) {
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
            // Preserve a lightweight placeholder rather than silently dropping
            // non-text content that this API cannot currently serve.
            let label = data
                .get("filename")
                .or_else(|| data.get("url"))
                .and_then(Value::as_str);
            if let Some(label) = label {
                add_part(parent, text_part(&format!("[{label}]")));
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
        for part in parts {
            parse_part(&mut parent, part, &mut results);
        }
    }
    if !parent.content.is_empty() || parent.parts.as_ref().is_some_and(|p| !p.is_empty()) {
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
        for part in parts {
            let raw: String = part.try_get("data")?;
            if let Ok(value) = serde_json::from_str::<Value>(&raw) {
                parse_part(&mut parent, &value, &mut results);
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
    fn parses_text_thinking_and_tool_result_as_separate_messages() {
        let data: Value = serde_json::json!({
            "role":"assistant",
            "modelID":"claude-sonnet",
            "tokens":{"input":12,"output":5},
            "finish":"tool-calls",
            "time":{"created":1793700000000},
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
    fn retains_running_tool_call_without_fabricating_result() {
        let mut parent = message("m1".into(), "assistant", None);
        let mut results = Vec::new();
        parse_part(
            &mut parent,
            &serde_json::json!({
                "type":"tool","callID":"c1","tool":"bash",
                "state":{"status":"running","input":{"command":"pwd"}}
            }),
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
        let options = SqliteConnectOptions::new().filename(&path).create_if_missing(true);
        let mut conn = sqlx::SqliteConnection::connect_with(&options).await.unwrap();
        sqlx::query("CREATE TABLE session_message (id TEXT, session_id TEXT, type TEXT, seq INTEGER, time_created INTEGER, time_updated INTEGER, data TEXT)")
            .execute(&mut conn).await.unwrap();
        sqlx::query("CREATE TABLE session_part (message_id TEXT, session_id TEXT, ordinal INTEGER, type TEXT, call_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT)")
            .execute(&mut conn).await.unwrap();

        sqlx::query("INSERT INTO session_message VALUES (?, ?, ?, ?, ?, ?, ?)")
            .bind("msg-user").bind("ses_v2").bind("user").bind(1_i64)
            .bind(1793700000000_i64).bind(1793700000000_i64)
            .bind(r#"{"text":"hello from OpenCode","time":{"created":1793700000000}}"#)
            .execute(&mut conn).await.unwrap();
        sqlx::query("INSERT INTO session_message VALUES (?, ?, ?, ?, ?, ?, ?)")
            .bind("msg-v2").bind("ses_v2").bind("assistant").bind(2_i64)
            .bind(1793700001000_i64).bind(1793700001000_i64)
            .bind(r#"{"model":{"providerID":"anthropic","id":"claude-sonnet-4-5"},"tokens":{"input":12,"output":5},"finish":"tool-calls","content":[],"time":{"created":1793700001000}}"#)
            .execute(&mut conn).await.unwrap();

        for (ordinal, kind, call_id, data) in [
            (0_i64, "text", None, r#"{"type":"text","text":"v2 fixture"}"#),
            (1_i64, "reasoning", None, r#"{"type":"reasoning","text":"thinking"}"#),
            (2_i64, "tool", Some("call-v2"), r#"{"type":"tool","id":"call-v2","name":"read","state":{"status":"error","input":{"file":"x"},"content":[{"type":"text","text":"not found"}],"error":{"message":"read failed"}},"time":{"created":1793700001001}}"#),
        ] {
            sqlx::query("INSERT INTO session_part VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
                .bind("msg-v2").bind("ses_v2").bind(ordinal).bind(kind).bind(call_id)
                .bind(1793700001001_i64).bind(1793700001001_i64).bind(data)
                .execute(&mut conn).await.unwrap();
        }
        conn.close().await.unwrap();

        let messages = read_messages_from(&path, "ses_v2").await.unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "hello from OpenCode");
        assert_eq!(messages[1].content, "v2 fixture");
        assert_eq!(messages[1].model.as_deref(), Some("anthropic/claude-sonnet-4-5"));
        assert_eq!(messages[1].parts.as_ref().unwrap()[1].kind, "thinking");
        assert_eq!(messages[2].role, "toolResult");
        assert_eq!(messages[2].content, "not found");
        assert_eq!(messages[2].is_error, Some(true));
        assert_eq!(messages[2].error_message.as_deref(), Some("read failed"));
        let _ = std::fs::remove_file(path);
    }
}
