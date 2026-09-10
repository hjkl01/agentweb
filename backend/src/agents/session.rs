use super::{adapter::AgentRunError, AgentManager};
use crate::{api::{build_agent_config, Session}, events::{AgentEvent, EventBus}};
use chrono::Utc;
use std::{path::{Component, Path, PathBuf}, sync::Arc};
use tokio::{fs, time::{self, Duration}};

const MAX_REFERENCE_FILES: usize = 100;
const MAX_REFERENCE_FILE_BYTES: u64 = 64 * 1024;
const MAX_REFERENCE_TOTAL_BYTES: usize = 512 * 1024;

async fn finish_session(db: &sqlx::SqlitePool, session_id: &str, status: &str) {
    let _ = sqlx::query("UPDATE sessions SET status=?,updated_at=? WHERE id=? AND status='running'")
        .bind(status).bind(Utc::now().to_rfc3339()).bind(session_id).execute(db).await;
}

async fn persist_stream(db: sqlx::SqlitePool, session_id: String, message_id: String, events: EventBus) {
    let mut rx = events.subscribe();
    let mut pending = String::new();
    let mut ticker = time::interval(Duration::from_millis(100));
    loop {
        tokio::select! {
            result = rx.recv() => match result {
                Ok(AgentEvent::MessageDelta { session_id: id, text }) if id == session_id => pending.push_str(&text),
                Ok(AgentEvent::MessageCompleted { session_id: id }) | Ok(AgentEvent::SessionCompleted { session_id: id }) if id == session_id => {
                    if !pending.is_empty() {
                        let _ = sqlx::query("UPDATE messages SET content=content||? WHERE id=?")
                            .bind(&pending).bind(&message_id).execute(&db).await;
                    }
                    return;
                }
                Ok(_) => {}
                Err(_) => return,
            },
            _ = ticker.tick() => {
                if !pending.is_empty() {
                    let chunk = std::mem::take(&mut pending);
                    if sqlx::query("UPDATE messages SET content=content||? WHERE id=?")
                        .bind(&chunk).bind(&message_id).execute(&db).await.is_err() { return; }
                }
            }
        }
    }
}

fn mention_paths(message: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for token in message.split_whitespace() {
        let Some(path) = token.strip_prefix('@') else { continue };
        let path = path.trim_matches(|c: char| matches!(c, ',' | '.' | ':' | ';' | ')' | ']' | '}'));
        if path.is_empty() || path == "@" || path.starts_with('@') || path.contains('\0') { continue; }
        if !paths.iter().any(|existing| existing == path) { paths.push(path.to_owned()); }
    }
    paths
}

fn safe_relative(path: &str) -> Option<PathBuf> {
    let path = Path::new(path);
    if path.is_absolute() { return None; }
    for component in path.components() {
        match component {
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
            Component::CurDir => {}
            Component::Normal(_) => {}
        }
    }
    Some(path.to_path_buf())
}

async fn collect_reference_files(root: &Path, path: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if files.len() >= MAX_REFERENCE_FILES { return Ok(()); }
    let candidate = root.join(path);
    let canonical = fs::canonicalize(&candidate).await?;
    if !canonical.starts_with(root) { return Ok(()); }
    let metadata = fs::metadata(&canonical).await?;
    if metadata.is_file() {
        files.push(canonical);
        return Ok(());
    }
    if !metadata.is_dir() { return Ok(()); }
    let mut entries = fs::read_dir(&canonical).await?;
    while let Some(entry) = entries.next_entry().await? {
        if files.len() >= MAX_REFERENCE_FILES { break; }
        let name = entry.file_name().to_string_lossy().into_owned();
        if matches!(name.as_str(), ".git" | "node_modules" | "target") { continue; }
        let child = entry.path();
        let child_canonical = match fs::canonicalize(&child).await { Ok(p) => p, Err(_) => continue };
        if !child_canonical.starts_with(root) { continue; }
        let child_meta = match fs::metadata(&child_canonical).await { Ok(m) => m, Err(_) => continue };
        if child_meta.is_file() {
            files.push(child_canonical);
        } else if child_meta.is_dir() {
            let relative = match child_canonical.strip_prefix(root) { Ok(p) => p, Err(_) => continue };
            collect_reference_files(root, relative, files).await?;
        }
    }
    Ok(())
}

async fn expand_mentions(message: &str, workspace: &str) -> String {
    let mentions = mention_paths(message);
    if mentions.is_empty() { return message.to_owned(); }
    let Ok(root) = fs::canonicalize(workspace).await else { return message.to_owned(); };
    let mut output = String::with_capacity(message.len() + 4096);
    output.push_str(message);
    output.push_str("\n\n<agentweb_file_context>\n");
    let mut total_bytes = 0usize;
    let mut total_files = 0usize;
    let mut seen = std::collections::HashSet::new();

    for mention in mentions {
        let Some(relative) = safe_relative(&mention) else { continue };
        let mut files = Vec::new();
        if collect_reference_files(&root, &relative, &mut files).await.is_err() { continue; }
        for file in files {
            if total_files >= MAX_REFERENCE_FILES || total_bytes >= MAX_REFERENCE_TOTAL_BYTES { break; }
            if !seen.insert(file.clone()) { continue; }
            let metadata = match fs::metadata(&file).await { Ok(m) => m, Err(_) => continue };
            if metadata.len() > MAX_REFERENCE_FILE_BYTES { continue; }
            let content = match fs::read_to_string(&file).await { Ok(c) => c, Err(_) => continue };
            let remaining = MAX_REFERENCE_TOTAL_BYTES - total_bytes;
            let bytes = content.as_bytes();
            if bytes.len() > remaining { continue; }
            let relative_display = match file.strip_prefix(&root) {
                Ok(p) => p.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };
            output.push_str("\n<file path=\"");
            output.push_str(&relative_display.replace('"', "&quot;"));
            output.push_str("\">\n");
            output.push_str(&content);
            if !content.ends_with('\n') { output.push('\n'); }
            output.push_str("</file>\n");
            total_bytes += bytes.len();
            total_files += 1;
        }
        if total_files >= MAX_REFERENCE_FILES || total_bytes >= MAX_REFERENCE_TOTAL_BYTES { break; }
    }
    output.push_str("</agentweb_file_context>");
    if total_files == 0 { message.to_owned() } else { output }
}

pub async fn run_session(db: sqlx::SqlitePool, agents: Arc<AgentManager>, session: Session, message: String, events: EventBus) {
    let mut config = match build_agent_config(&db, &session.agent_id).await {
        Ok(config) => config,
        Err(_) => {
            finish_session(&db, &session.id, "error").await;
            events.publish(AgentEvent::Error { session_id: session.id, message: "agent executable not found".into() });
            return;
        }
    };
    config.working_directory = Some(session.workspace.clone());
    config.native_session_id = session.native_session_id.clone();
    config.model = session.model.clone();
    let kind = config.id.clone();
    let message = expand_mentions(&message, &session.workspace).await;
    let assistant_id = uuid::Uuid::new_v4().to_string();
    let created_at = Utc::now().to_rfc3339();
    if sqlx::query("INSERT INTO messages(id,session_id,role,content,created_at) VALUES(?,?,?,?,?)")
        .bind(&assistant_id).bind(&session.id).bind("assistant").bind("").bind(&created_at).execute(&db).await.is_err() {
        finish_session(&db, &session.id, "error").await;
        events.publish(AgentEvent::Error { session_id: session.id, message: "failed to create assistant message".into() });
        return;
    }
    let persist_task = tokio::spawn(persist_stream(db.clone(), session.id.clone(), assistant_id.clone(), events.clone()));
    let adapter = agents.adapter(&kind).await;
    match adapter.send_message(&config, &session.id, &message, &events).await {
        Ok(result) => {
            persist_task.abort();
            let now = Utc::now().to_rfc3339();
            let updated = sqlx::query("UPDATE sessions SET native_session_id=COALESCE(?,native_session_id),status='idle',updated_at=? WHERE id=? AND status='running'")
                .bind(&result.native_session_id).bind(&now).bind(&session.id).execute(&db).await;
            if updated.map(|r| r.rows_affected() == 1).unwrap_or(false) && !result.assistant_text.is_empty() {
                let _ = sqlx::query("UPDATE messages SET content=? WHERE id=?")
                    .bind(result.assistant_text).bind(&assistant_id).execute(&db).await;
            }
        }
        Err(error) => {
            persist_task.abort();
            let interrupted = error.downcast_ref::<AgentRunError>().is_some();
            finish_session(&db, &session.id, if interrupted { "interrupted" } else { "error" }).await;
            let content: Option<String> = sqlx::query_scalar("SELECT content FROM messages WHERE id=?").bind(&assistant_id).fetch_optional(&db).await.unwrap_or(None);
            if content.as_deref().map(str::trim).unwrap_or_default().is_empty() {
                let _ = sqlx::query("DELETE FROM messages WHERE id=?").bind(&assistant_id).execute(&db).await;
            }
            if !interrupted { events.publish(AgentEvent::Error { session_id: session.id, message: error.to_string() }); }
        }
    }
}
