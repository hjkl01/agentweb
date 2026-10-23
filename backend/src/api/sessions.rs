use crate::state::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::path::{Component, Path as FsPath, PathBuf};
use tokio::fs;
use uuid::Uuid;

#[derive(Serialize, Clone)]
pub struct Session {
    pub id: String,
    pub agent_id: String,
    pub title: String,
    pub workspace: String,
    pub status: String,
    pub status_updated_at: String,
    pub native_session_id: Option<String>,
    pub fork_from_native_session_id: Option<String>,
    pub model: Option<String>,
    pub thinking_level: Option<String>,
    pub tool_preset: Option<String>,
    pub is_pinned: bool,
    pub archived_at: Option<String>,
    pub workspace_available: bool,
}
fn session_from_row(row: &sqlx::sqlite::SqliteRow) -> Session {
    Session {
        id: row.get("id"),
        agent_id: row.get("agent_id"),
        title: row.get("title"),
        workspace: row.get("workspace"),
        status: row.get("status"),
        status_updated_at: row.get("status_updated_at"),
        native_session_id: row.get("native_session_id"),
        model: row.get("model"),
        thinking_level: row.get("thinking_level"),
        tool_preset: row.get("tool_preset"),
        is_pinned: row.get::<i64, _>("is_pinned") != 0,
        archived_at: row.get("archived_at"),
        fork_from_native_session_id: row.get("fork_from_native_session_id"),
        workspace_available: false,
    }
}
fn default_workspace_root() -> PathBuf {
    if let Ok(path) = std::env::var("AGENTWEB_WORKSPACE_DIR") {
        return PathBuf::from(path);
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub(super) async fn canonical_home() -> Result<PathBuf, StatusCode> {
    fs::canonicalize(default_workspace_root())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// Split a requested path at a leading `~`/`$HOME` component and return the
/// remainder. `~/github/agentweb` means "github/agentweb under home", so it must
/// never resolve to a literal `~` directory.
fn home_remainder(value: &str) -> Option<&str> {
    let mut offset = 0;
    for part in value.split('/') {
        if part == "~" || part == "$HOME" {
            return Some(value[offset + part.len()..].trim_start_matches('/'));
        }
        offset += part.len() + 1;
    }
    None
}

/// Resolve a requested workspace path against `home`, expanding `~`/`$HOME`.
fn workspace_candidate(value: &str, home: &FsPath) -> PathBuf {
    match home_remainder(value) {
        Some("") => home.to_path_buf(),
        Some(rest) => home.join(rest),
        None if FsPath::new(value).is_absolute() => PathBuf::from(value),
        None => home.join(value),
    }
}
pub(crate) async fn load_session_record(
    db: &sqlx::SqlitePool,
    id: &str,
) -> Result<Session, StatusCode> {
    let row=sqlx::query("SELECT s.id,s.agent_id,s.title,s.workspace,r.status,r.updated_at AS status_updated_at,s.native_session_id,s.model,s.thinking_level,s.tool_preset,s.is_pinned,s.fork_from_native_session_id,s.archived_at FROM sessions s JOIN session_runtime r ON r.session_id=s.id WHERE s.id=?").bind(id).fetch_optional(db).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?.ok_or(StatusCode::NOT_FOUND)?;
    Ok(session_from_row(&row))
}

pub(crate) async fn load_session(db: &sqlx::SqlitePool, id: &str) -> Result<Session, StatusCode> {
    let mut session = load_session_record(db, id).await?;

    let workspace = fs::canonicalize(&session.workspace)
        .await
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::FORBIDDEN
            }
        })?;
    if !workspace.is_dir() {
        return Err(StatusCode::FORBIDDEN);
    }
    session.workspace = workspace.to_string_lossy().into_owned();
    session.workspace_available = true;
    Ok(session)
}
#[derive(Deserialize, Default)]
pub struct WorkspaceBrowserQuery {
    pub path: Option<String>,
}

pub async fn list_workspaces(
    Query(query): Query<WorkspaceBrowserQuery>,
) -> Result<Json<Vec<String>>, StatusCode> {
    let home = canonical_home().await?;
    let current = match query.path.as_deref().map(str::trim) {
        None | Some("") | Some(".") => home.clone(),
        Some(path) => {
            let requested = workspace_candidate(path, &home);
            let candidate = fs::canonicalize(requested)
                .await
                .map_err(|_| StatusCode::NOT_FOUND)?;
            if !candidate.is_dir() || !candidate.starts_with(&home) {
                return Err(StatusCode::FORBIDDEN);
            }
            candidate
        }
    };
    let mut entries = fs::read_dir(&current)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut dirs = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        let kind = match entry.file_type().await {
            Ok(v) => v,
            Err(_) => continue,
        };
        if !kind.is_dir() || kind.is_symlink() {
            continue;
        }
        if let Ok(path) = fs::canonicalize(entry.path()).await {
            if path.starts_with(&home) {
                dirs.push(path.to_string_lossy().into_owned());
            }
        }
    }
    dirs.sort_by_key(|p| p.to_lowercase());
    let mut result = Vec::with_capacity(dirs.len() + 1);
    result.push(current.to_string_lossy().into_owned());
    result.extend(dirs);
    Ok(Json(result))
}

pub async fn list_sessions(State(s): State<AppState>) -> Result<Json<Vec<Session>>, StatusCode> {
    // A failed discovery task is an internal error, not an empty session list:
    // silently treating a worker panic as "no native sessions" hides history.
    let discovered =
        tokio::task::spawn_blocking(crate::agents::pi_transcript::list_native_sessions)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    // Incomplete discovery may still contribute found sessions, but must never be
    // interpreted as proof that any previously imported native session was deleted.
    // Emit a diagnostic so operators can distinguish an empty store from a failed
    // or safety-limited scan without hiding the sessions we did successfully find.
    if !discovered.complete {
        tracing::warn!(
            discovered_sessions = discovered.sessions.len(),
            "Pi native session discovery was incomplete; preserving existing session metadata"
        );
    }
    for native in discovered.sessions {
        let id = Uuid::new_v4().to_string();
        // Keep discovery idempotent across multiple backend workers: only
        // insert metadata when this native session has not been imported yet.
        sqlx::query(
            "INSERT INTO sessions(id,agent_id,title,workspace,status,native_session_id,model,is_pinned,created_at,updated_at) SELECT ?, 'pi', ?, ?, 'idle', ?, NULL, 0, ?, ? WHERE NOT EXISTS (SELECT 1 FROM sessions WHERE agent_id='pi' AND native_session_id=?)",
        )
        .bind(id)
        .bind(&native.title)
        .bind(&native.cwd)
        .bind(&native.id)
        .bind(&native.created_at)
        .bind(&native.updated_at)
        .bind(&native.id)
        .execute(&s.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        // Explicitly provision runtime state for imported sessions. The insert
        // is idempotent if another discovery worker already created this row.
        sqlx::query(
            "INSERT OR IGNORE INTO session_runtime(session_id,status,worker_id,lease_until,updated_at) SELECT id,'idle',NULL,NULL,updated_at FROM sessions WHERE agent_id='pi' AND native_session_id=?",
        )
        .bind(&native.id)
        .execute(&s.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    let rows=sqlx::query("SELECT s.id,s.agent_id,s.title,s.workspace,r.status,r.updated_at AS status_updated_at,s.native_session_id,s.model,s.thinking_level,s.tool_preset,s.is_pinned,s.fork_from_native_session_id,s.archived_at FROM sessions s JOIN session_runtime r ON r.session_id=s.id ORDER BY s.is_pinned DESC, s.updated_at DESC")
        .fetch_all(&s.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut sessions = Vec::with_capacity(rows.len());
    for row in &rows {
        let mut session = session_from_row(row);
        if let Ok(workspace) = fs::canonicalize(&session.workspace).await {
            session.workspace_available = workspace.is_dir();
            if session.workspace_available {
                session.workspace = workspace.to_string_lossy().into_owned();
            }
        }
        sessions.push(session);
    }
    Ok(Json(sessions))
}

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: String,
}

#[derive(Serialize)]
pub struct SessionSearchHit {
    pub session: Session,
    pub entry_id: String,
    pub role: String,
    pub before: String,
    pub matched: String,
    pub after: String,
}

#[derive(Serialize)]
pub struct SessionSearchResponse {
    pub results: Vec<SessionSearchHit>,
    /// The scan hit a cap or its time budget, so older sessions were not searched.
    pub truncated: bool,
}

/// Conversation content lives in the Agent's own session store, so search has to read it there.
const SEARCH_CANDIDATE_LIMIT: i64 = 500;
const SEARCH_BUDGET: std::time::Duration = std::time::Duration::from_secs(3);
const SEARCH_QUERY_MAX_CHARS: usize = 200;

pub async fn search_sessions(
    Query(query): Query<SearchQuery>,
    State(s): State<AppState>,
) -> Result<Json<SessionSearchResponse>, (StatusCode, String)> {
    let needle = query.q.trim().to_owned();
    if needle.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "搜索关键词不能为空".to_owned()));
    }
    if needle.chars().count() > SEARCH_QUERY_MAX_CHARS {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("搜索关键词最多 {SEARCH_QUERY_MAX_CHARS} 个字符"),
        ));
    }
    // Newest first: the transcript scan is capped, so recency decides what is covered.
    let rows = sqlx::query(
        "SELECT s.id,s.agent_id,s.title,s.workspace,r.status,r.updated_at AS status_updated_at,s.native_session_id,s.model,s.thinking_level,s.tool_preset,s.is_pinned,s.fork_from_native_session_id,s.archived_at FROM sessions s JOIN session_runtime r ON r.session_id=s.id WHERE s.native_session_id IS NOT NULL ORDER BY s.updated_at DESC LIMIT ?",
    )
    .bind(SEARCH_CANDIDATE_LIMIT)
    .fetch_all(&s.db)
    .await
    .map_err(|error| {
        tracing::error!(%error, "failed to list sessions for search");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "查询会话失败".to_owned(),
        )
    })?;
    let mut sessions = Vec::with_capacity(rows.len());
    let mut native_ids = Vec::with_capacity(rows.len());
    for row in &rows {
        let session = session_from_row(row);
        if let Some(native_session_id) = session.native_session_id.clone() {
            native_ids.push(native_session_id);
        }
        sessions.push(session);
    }
    let permit = super::session_messages::TRANSCRIPT_READ_LIMIT
        .acquire()
        .await
        .map_err(|error| {
            tracing::error!(%error, "transcript read limiter is unavailable");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "会话内容正在被其他请求占用".to_owned(),
            )
        })?;
    let (hits, truncated) = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        crate::agents::pi_transcript::search_transcripts(&native_ids, &needle, SEARCH_BUDGET)
    })
    .await
    .map_err(|error| {
        tracing::error!(%error, "session search task failed");
        (StatusCode::INTERNAL_SERVER_ERROR, "搜索失败".to_owned())
    })?;
    let by_native: std::collections::HashMap<&str, &Session> = sessions
        .iter()
        .filter_map(|session| Some((session.native_session_id.as_deref()?, session)))
        .collect();
    let results = hits
        .into_iter()
        .filter_map(|hit| {
            let session = by_native.get(hit.session_id.as_str())?;
            Some(SessionSearchHit {
                session: (*session).clone(),
                entry_id: hit.entry_id,
                role: hit.role,
                before: hit.before,
                matched: hit.matched,
                after: hit.after,
            })
        })
        .collect();
    Ok(Json(SessionSearchResponse { results, truncated }))
}
#[derive(Deserialize)]
pub struct CreateSession {
    pub agent_id: Option<String>,
    pub title: Option<String>,
    pub workspace: Option<String>,
    pub model: Option<String>,
    pub thinking_level: Option<String>,
    pub tool_preset: Option<String>,
}
async fn resolve_workspace(requested: Option<&str>) -> Result<String, StatusCode> {
    let value = requested.unwrap_or("").trim();
    let home = canonical_home().await?;
    if value.is_empty() || value == "." {
        return Ok(home.to_string_lossy().into_owned());
    }
    let candidate = workspace_candidate(value, &home);
    let path = fs::canonicalize(&candidate).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::BAD_REQUEST
        }
    })?;
    if !path.is_dir() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if !path.starts_with(&home) {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(path.to_string_lossy().into_owned())
}

async fn resolve_workspace_under(base_path: &FsPath, name: &str) -> Result<String, StatusCode> {
    fs::create_dir_all(base_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let base = fs::canonicalize(base_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if name.is_empty() || name == "." {
        return Err(StatusCode::BAD_REQUEST);
    }
    let path = FsPath::new(name);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Resolve/create one component at a time. create_dir_all on an unchecked
    // nested path could follow an existing symlink and create directories
    // outside the configured workspace root before the final check.
    let mut current = base.clone();
    for component in path.components() {
        current.push(component.as_os_str());
        match fs::metadata(&current).await {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => return Err(StatusCode::BAD_REQUEST),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current)
                    .await
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            }
            Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
        }
        let canonical = fs::canonicalize(&current)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if !canonical.starts_with(&base) {
            return Err(StatusCode::FORBIDDEN);
        }
        current = canonical;
    }
    Ok(current.to_string_lossy().into_owned())
}

#[derive(Deserialize)]
pub struct CreateWorkspaceDir {
    pub parent: Option<String>,
    pub name: String,
}

/// Create a directory under an existing workspace directory and return its
/// canonical path. The parent must already resolve inside the workspace root;
/// `resolve_workspace_under` then creates each component with per-component
/// symlink checks so a pre-existing link cannot escape the root.
pub async fn create_workspace(
    Json(input): Json<CreateWorkspaceDir>,
) -> Result<Json<String>, StatusCode> {
    let home = canonical_home().await?;
    let parent = match input.parent.as_deref().map(str::trim) {
        Some(value) if !value.is_empty() => fs::canonicalize(value)
            .await
            .map_err(|_| StatusCode::NOT_FOUND)?,
        _ => home.clone(),
    };
    if !parent.is_dir() || !parent.starts_with(&home) {
        return Err(StatusCode::FORBIDDEN);
    }
    let name = input.name.trim();
    if home_remainder(name).is_some() {
        // `~/x` is a path relative to home, not a folder name; accepting it here
        // is how literal `~` workspace directories got created.
        return Err(StatusCode::BAD_REQUEST);
    }
    let path = resolve_workspace_under(&parent, name).await?;
    Ok(Json(path))
}

/// LIKE pattern for "the target itself or anything under it". `workspace` is a
/// canonical directory path, but `_` and `%` are still LIKE wildcards (and
/// SQLite LIKE is case-insensitive), so a naive `path + "/%"` would treat
/// `/home/u/my_app` as matching `/home/u/myXapp` and wrongly block (or, with a
/// literal `%` in a name, wrongly allow) the delete.
fn workspace_like_prefix(path: &str) -> String {
    let escaped = path
        .replace('\\', "\\\\")
        .replace('_', "\\_")
        .replace('%', "\\%");
    format!("{escaped}/%")
}

#[derive(Deserialize)]
pub struct DeleteWorkspaceDir {
    pub path: String,
}

fn workspace_error(
    status: StatusCode,
    code: &str,
    message: impl Into<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({"error": {"code": code, "message": message.into()}})),
    )
}

/// Delete one workspace directory under the workspace root, recursively.
///
/// Refused while any session still points at it (or into it): the session rows
/// are the only record of the work done there, and removing the directory would
/// leave them pointing at nothing.
pub async fn delete_workspace(
    State(s): State<AppState>,
    Json(input): Json<DeleteWorkspaceDir>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let home = canonical_home()
        .await
        .map_err(|status| workspace_error(status, "WORKSPACE", "无法解析工作区根目录"))?;
    let target = fs::canonicalize(input.path.trim())
        .await
        .map_err(|_| workspace_error(StatusCode::NOT_FOUND, "WORKSPACE", "目录不存在"))?;
    if !target.is_dir() || !target.starts_with(&home) {
        return Err(workspace_error(
            StatusCode::FORBIDDEN,
            "WORKSPACE",
            "目录不在允许的工作区根目录内",
        ));
    }
    // The root itself is the whole workspace tree, never a workspace.
    if target == home {
        return Err(workspace_error(
            StatusCode::BAD_REQUEST,
            "WORKSPACE",
            "不能删除工作区根目录",
        ));
    }

    let target_path = target.to_string_lossy().into_owned();
    let in_use: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sessions WHERE workspace = ?1 OR workspace LIKE ?2 ESCAPE '\\'",
    )
    .bind(&target_path)
    .bind(workspace_like_prefix(&target_path))
    .fetch_one(&s.db)
    .await
    .map_err(|_| {
        workspace_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "WORKSPACE",
            "查询会话失败",
        )
    })?;
    if in_use > 0 {
        return Err(workspace_error(
            StatusCode::CONFLICT,
            "WORKSPACE_IN_USE",
            format!("该目录下还有 {in_use} 个会话，请先删除这些会话"),
        ));
    }

    fs::remove_dir_all(&target).await.map_err(|_| {
        workspace_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "WORKSPACE",
            "删除目录失败",
        )
    })?;
    Ok(Json(serde_json::json!({"success": true})))
}

pub async fn create_session(
    State(s): State<AppState>,
    Json(v): Json<CreateSession>,
) -> Result<Json<Session>, StatusCode> {
    let agent_id = match v.agent_id.filter(|v| !v.trim().is_empty()) {
        Some(id) => id,
        None => crate::api::runtime::setting(&s.db, "default_agent_id")
            .await
            .filter(|v| !v.trim().is_empty())
            .ok_or(StatusCode::BAD_REQUEST)?,
    };
    crate::api::runtime::build_agent_config(&s.db, &agent_id)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let model = match v.model {
        Some(model) => Some(model.trim().to_owned()).filter(|m| !m.is_empty()),
        None => crate::api::runtime::setting(&s.db, "default_model")
            .await
            .filter(|v| !v.trim().is_empty()),
    };
    let thinking_level = match v.thinking_level {
        Some(raw) if !raw.trim().is_empty() => Some(
            crate::agents::adapter::normalize_thinking_level(&raw)
                .ok_or(StatusCode::BAD_REQUEST)?,
        ),
        _ => None,
    };
    // Tools stay on the Agent's default unless the caller picks a preset; the same rules as
    // the dedicated endpoint apply here.
    let tool_preset = match v.tool_preset {
        Some(raw) if !raw.trim().is_empty() => Some(
            crate::agents::adapter::normalize_tool_preset(&raw).ok_or(StatusCode::BAD_REQUEST)?,
        ),
        _ => None,
    };
    let id = Uuid::new_v4().to_string();
    let workspace = resolve_workspace(v.workspace.as_deref()).await?;
    let now = Utc::now().to_rfc3339();
    let title = v
        .title
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| "New Chat".into());
    let mut tx =
        s.db.begin()
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    sqlx::query("INSERT INTO sessions(id,agent_id,title,workspace,status,native_session_id,model,thinking_level,tool_preset,is_pinned,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)").bind(&id).bind(&agent_id).bind(&title).bind(&workspace).bind("idle").bind::<Option<String>>(None).bind(&model).bind(&thinking_level).bind(&tool_preset).bind(0i64).bind(&now).bind(&now).execute(&mut *tx).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    sqlx::query("INSERT INTO session_runtime(session_id,status,worker_id,lease_until,updated_at) VALUES(?,'idle',NULL,NULL,?)")
        .bind(&id).bind(&now).execute(&mut *tx).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tx.commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(Session {
        id,
        agent_id,
        title,
        workspace,
        status: "idle".into(),
        status_updated_at: now.clone(),
        native_session_id: None,
        fork_from_native_session_id: None,
        model,
        thinking_level,
        tool_preset,
        is_pinned: false,
        archived_at: None,
        workspace_available: true,
    }))
}
#[derive(Deserialize)]
pub struct RenameSession {
    pub title: String,
}
pub async fn rename_session(
    Path(id): Path<String>,
    State(s): State<AppState>,
    Json(v): Json<RenameSession>,
) -> Result<Json<Session>, StatusCode> {
    let title = v.title.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let title = title.chars().take(80).collect::<String>();
    load_session(&s.db, &id).await?;
    sqlx::query("UPDATE sessions SET title=?,updated_at=? WHERE id=?")
        .bind(&title)
        .bind(Utc::now().to_rfc3339())
        .bind(&id)
        .execute(&s.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(load_session(&s.db, &id).await?))
}
pub async fn toggle_pin(
    Path(id): Path<String>,
    State(s): State<AppState>,
) -> Result<Json<Session>, StatusCode> {
    load_session(&s.db, &id).await?;
    sqlx::query("UPDATE sessions SET is_pinned=CASE WHEN is_pinned=1 THEN 0 ELSE 1 END,updated_at=? WHERE id=?").bind(Utc::now().to_rfc3339()).bind(&id).execute(&s.db).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(load_session(&s.db, &id).await?))
}

#[derive(Deserialize)]
pub struct ArchiveBody {
    pub archived: bool,
}

/// Archive hides a session from its project group without deleting anything. An explicit
/// value (not a toggle) so「取消归档」is idempotent and cannot race the archive view.
pub async fn set_session_archived(
    Path(id): Path<String>,
    State(s): State<AppState>,
    Json(body): Json<ArchiveBody>,
) -> Result<Json<Session>, StatusCode> {
    load_session(&s.db, &id).await?;
    let now = Utc::now().to_rfc3339();
    let archived_at = body.archived.then(|| now.clone());
    sqlx::query("UPDATE sessions SET archived_at=?,updated_at=? WHERE id=?")
        .bind(archived_at)
        .bind(now)
        .bind(&id)
        .execute(&s.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(load_session(&s.db, &id).await?))
}

pub async fn get_session(
    Path(id): Path<String>,
    State(s): State<AppState>,
) -> Result<Json<Session>, StatusCode> {
    let mut session = load_session_record(&s.db, &id).await?;
    if let Ok(workspace) = fs::canonicalize(&session.workspace).await {
        session.workspace_available = workspace.is_dir();
        if session.workspace_available {
            session.workspace = workspace.to_string_lossy().into_owned();
        }
    }
    Ok(Json(session))
}

/// Resolves a Pi session that can be forked: idle, and with its native session file really on
/// disk. A missing file marks the session for recovery, exactly like a transcript read does.
async fn forkable_pi_source(
    db: &sqlx::SqlitePool,
    id: &str,
) -> Result<(Session, String), StatusCode> {
    let source = load_session(db, id).await?;
    if source.status == "running"
        || source.status == "missing"
        || source.native_session_id.is_none()
    {
        return Err(StatusCode::CONFLICT);
    }
    let kind: Option<String> = sqlx::query_scalar("SELECT kind FROM agents WHERE id=?")
        .bind(&source.agent_id)
        .fetch_optional(db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if kind.as_deref() != Some("pi") {
        return Err(StatusCode::BAD_REQUEST);
    }
    let native_id = source
        .native_session_id
        .as_deref()
        .ok_or(StatusCode::CONFLICT)?
        .to_owned();
    let lookup = {
        let native_id = native_id.clone();
        tokio::task::spawn_blocking(move || {
            crate::agents::pi_transcript::native_session_file_exists(&native_id)
        })
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    };
    match lookup {
        crate::agents::pi_transcript::NativeSessionFileLookup::Found => {}
        crate::agents::pi_transcript::NativeSessionFileLookup::Unknown => {
            tracing::warn!(session_id = %id, native_session_id = %native_id, "Pi transcript lookup was inconclusive during fork; preserving session metadata");
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
        crate::agents::pi_transcript::NativeSessionFileLookup::Missing => {
            sqlx::query("UPDATE session_runtime SET status='missing',updated_at=? WHERE session_id=? AND status!='running' AND worker_id IS NULL AND EXISTS (SELECT 1 FROM sessions s WHERE s.id=session_runtime.session_id AND s.native_session_id=?)")
                .bind(Utc::now().to_rfc3339())
                .bind(id)
                .bind(&native_id)
                .execute(db)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            return Err(StatusCode::CONFLICT);
        }
    }
    let source_still_available: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions s JOIN session_runtime r ON r.session_id=s.id WHERE s.id=? AND s.native_session_id=? AND r.status!='running' AND r.worker_id IS NULL")
        .bind(id).bind(&native_id).fetch_one(db).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if source_still_available != 1 {
        return Err(StatusCode::CONFLICT);
    }
    Ok((source, native_id))
}

/// Adds a session that continues another Pi session, copying agent, workspace, model and
/// thinking level. `native_session_id` continues an already forked native session; `fork_from`
/// instead defers a leaf fork to the first message (see `agents::commands`).
async fn insert_forked_session(
    db: &sqlx::SqlitePool,
    source: &Session,
    native_session_id: Option<&str>,
    fork_from_native_session_id: Option<&str>,
) -> Result<Session, StatusCode> {
    let new_id = Uuid::new_v4().to_string();
    let title = format!(
        "分支：{}",
        source.title.chars().take(70).collect::<String>()
    );
    let now = Utc::now().to_rfc3339();
    let mut tx = db
        .begin()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    sqlx::query("INSERT INTO sessions(id,agent_id,title,workspace,status,native_session_id,model,thinking_level,tool_preset,is_pinned,created_at,updated_at,fork_from_native_session_id) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)")
        .bind(&new_id).bind(&source.agent_id).bind(&title).bind(&source.workspace)
        .bind("idle").bind(native_session_id).bind(&source.model).bind(&source.thinking_level).bind(&source.tool_preset).bind(0i64)
        .bind(&now).bind(&now).bind(fork_from_native_session_id).execute(&mut *tx).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    sqlx::query("INSERT INTO session_runtime(session_id,status,worker_id,lease_until,updated_at) VALUES(?,'idle',NULL,NULL,?)")
        .bind(&new_id).bind(&now).execute(&mut *tx).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tx.commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    load_session(db, &new_id).await
}

pub async fn create_pi_fork(
    Path(id): Path<String>,
    State(s): State<AppState>,
) -> Result<Json<Session>, StatusCode> {
    let (source, native_id) = forkable_pi_source(&s.db, &id).await?;
    Ok(Json(
        insert_forked_session(&s.db, &source, None, Some(&native_id)).await?,
    ))
}

#[derive(Deserialize)]
pub struct ForkFromEntry {
    pub entry_id: String,
}

#[derive(Serialize)]
pub struct ForkedSession {
    pub session: Session,
    /// The forked message, so the client can put it back in the editor.
    pub text: Option<String>,
}

/// Branches a Pi session at one of its user messages ("edit from here"). Pi performs the fork
/// itself, so the new session already owns its history and a native id, instead of copying the
/// source history at the next message like `create_pi_fork`. The source session is untouched.
pub async fn fork_session_at_entry(
    Path(id): Path<String>,
    State(s): State<AppState>,
    Json(request): Json<ForkFromEntry>,
) -> Result<Json<ForkedSession>, (StatusCode, String)> {
    let (source, native_id) = forkable_pi_source(&s.db, &id)
        .await
        .map_err(|status| (status, "会话当前无法分叉".to_owned()))?;
    let entry_id = request.entry_id.trim();
    if entry_id.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "缺少要分叉的消息".to_owned()));
    }
    // Pi forks user messages only, so reject anything else before starting its runtime.
    let forkable = {
        let native_id = native_id.clone();
        let entry_id = entry_id.to_owned();
        tokio::task::spawn_blocking(move || {
            crate::agents::pi_transcript::read_messages(&native_id)
                .into_iter()
                .any(|message| message.id == entry_id && message.role == "user")
        })
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "无法读取会话消息".to_owned(),
            )
        })?
    };
    if !forkable {
        return Err((StatusCode::BAD_REQUEST, "只能从用户消息开始分叉".to_owned()));
    }
    let mut config = crate::api::runtime::build_agent_config(&s.db, &source.agent_id)
        .await
        .map_err(|status| (status, "无法准备 Agent 配置".to_owned()))?;
    config.working_directory = Some(source.workspace.clone());
    config.native_session_id = Some(native_id);
    let fork = crate::agents::pi_rpc::fork_at_entry(&config, entry_id)
        .await
        .map_err(|error| (StatusCode::BAD_GATEWAY, error.to_string()))?;
    if fork.cancelled {
        return Err((StatusCode::CONFLICT, "分叉被取消".to_owned()));
    }
    // A fork before the first message has no history to copy, and Pi writes no file for it;
    // that branch is simply a new session.
    let forked = fork
        .session_file
        .as_ref()
        .filter(|path| path.is_file())
        .and(fork.session_id.as_deref());
    let session = insert_forked_session(&s.db, &source, forked, None)
        .await
        .map_err(|status| (status, "无法创建分叉会话".to_owned()))?;
    Ok(Json(ForkedSession {
        session,
        text: fork.text,
    }))
}

pub async fn restart_missing_session(
    Path(id): Path<String>,
    State(s): State<AppState>,
) -> Result<Json<Session>, StatusCode> {
    let now = Utc::now().to_rfc3339();
    let mut tx =
        s.db.begin()
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let runtime = sqlx::query(
        "UPDATE session_runtime SET status='idle',worker_id=NULL,lease_until=NULL,updated_at=? WHERE session_id=? AND status='missing' AND worker_id IS NULL",
    )
    .bind(&now)
    .bind(&id)
    .execute(&mut *tx)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if runtime.rows_affected() != 1 {
        tx.rollback()
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        load_session(&s.db, &id).await?;
        return Err(StatusCode::CONFLICT);
    }

    let metadata = sqlx::query(
        "UPDATE sessions SET native_session_id=NULL,fork_from_native_session_id=NULL,updated_at=? WHERE id=?",
    )
    .bind(&now)
    .bind(&id)
    .execute(&mut *tx)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if metadata.rows_affected() != 1 {
        tx.rollback()
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        return Err(StatusCode::NOT_FOUND);
    }
    tx.commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(load_session(&s.db, &id).await?))
}
pub async fn delete_session(Path(id): Path<String>, State(s): State<AppState>) -> StatusCode {
    let session = match load_session(&s.db, &id).await {
        Ok(v) => v,
        Err(StatusCode::NOT_FOUND) | Err(StatusCode::FORBIDDEN) => {
            // A project directory may have been removed manually, or an old
            // database row may point outside the permitted workspace root. Let users
            // clean up its orphaned, non-running session record without ever
            // interrupting a running execution whose workspace is unavailable.
            let row = match sqlx::query(
                "SELECT s.id,s.agent_id,s.title,s.workspace,r.status,r.updated_at AS status_updated_at,s.native_session_id,s.model,s.thinking_level,s.tool_preset,s.is_pinned,s.fork_from_native_session_id,s.archived_at FROM sessions s JOIN session_runtime r ON r.session_id=s.id WHERE s.id=?",
            )
            .bind(&id)
            .fetch_optional(&s.db)
            .await
            {
                Ok(Some(row)) => row,
                Ok(None) => return StatusCode::NOT_FOUND,
                Err(_) => return StatusCode::INTERNAL_SERVER_ERROR,
            };
            let orphan = session_from_row(&row);
            let base = fs::canonicalize(default_workspace_root()).await;
            let workspace = fs::canonicalize(&orphan.workspace).await;
            let workspace_is_valid = match (base, workspace) {
                (Ok(base), Ok(workspace)) => {
                    workspace.is_dir() && workspace.starts_with(&base) && workspace != base
                }
                _ => false,
            };
            if orphan.status == "running" || workspace_is_valid {
                return StatusCode::NOT_FOUND;
            }
            // Delete metadata and runtime state in one transaction. Without
            // the compatibility trigger, both records must be cleaned up
            // explicitly; the guarded DELETE also serializes against lease claims.
            let mut tx = match s.db.begin().await {
                Ok(tx) => tx,
                Err(_) => return StatusCode::INTERNAL_SERVER_ERROR,
            };
            let result = sqlx::query("DELETE FROM sessions WHERE id=? AND EXISTS (SELECT 1 FROM session_runtime r WHERE r.session_id=sessions.id AND r.status!='running')")
                .bind(&id)
                .execute(&mut *tx)
                .await;
            match result {
                Ok(result) if result.rows_affected() == 1 => {
                    if sqlx::query("DELETE FROM session_runtime WHERE session_id=?")
                        .bind(&id)
                        .execute(&mut *tx)
                        .await
                        .is_err()
                    {
                        let _ = tx.rollback().await;
                        return StatusCode::INTERNAL_SERVER_ERROR;
                    }
                    if tx.commit().await.is_err() {
                        return StatusCode::INTERNAL_SERVER_ERROR;
                    }
                    return StatusCode::NO_CONTENT;
                }
                Ok(_) => {
                    let _ = tx.rollback().await;
                    return StatusCode::CONFLICT;
                }
                Err(_) => {
                    let _ = tx.rollback().await;
                    return StatusCode::INTERNAL_SERVER_ERROR;
                }
            }
        }
        Err(e) => return e,
    };
    if session.status == "running" {
        // Never stop a live execution when deletion will be rejected anyway.
        // Only expired leases may be cleaned up by this endpoint.
        let execution: Option<(Option<String>, Option<String>)> = match sqlx::query_as(
            "SELECT worker_id,lease_until FROM session_runtime WHERE session_id=? AND status='running'",
        )
        .bind(&id)
        .fetch_optional(&s.db)
        .await
        {
            Ok(value) => value,
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR,
        };
        let Some((execution_id, lease_until)) = execution else {
            return StatusCode::CONFLICT;
        };
        if let Some(lease) = lease_until.as_deref() {
            match chrono::DateTime::parse_from_rfc3339(lease) {
                Ok(lease) if lease.with_timezone(&Utc) >= Utc::now() => {
                    return StatusCode::CONFLICT;
                }
                // Treat malformed lease timestamps conservatively: do not stop
                // or delete a possibly live execution based on invalid data.
                Err(_) => return StatusCode::CONFLICT,
                _ => {}
            }
        }
        let execution_id = execution_id;
        let Some(execution_id) = execution_id else {
            return StatusCode::CONFLICT;
        };
        let agent_kind: String = match sqlx::query_scalar("SELECT kind FROM agents WHERE id=?")
            .bind(&session.agent_id)
            .fetch_optional(&s.db)
            .await
        {
            Ok(value) => value.unwrap_or_else(|| session.agent_id.clone()),
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR,
        };
        let adapter = s.agents.adapter(&agent_kind).await;
        if adapter.interrupt(&id, &execution_id).await.is_err() {
            // Keep the session record if the agent process could not be stopped.
            return StatusCode::BAD_GATEWAY;
        }
    }
    // A send request may claim the session after the initial read/interrupt.
    // Keep live leases, but allow deleting a running record whose lease expired
    // (the owning worker's monitor will observe the missing row and interrupt).
    let now = Utc::now().to_rfc3339();
    let mut tx = match s.db.begin().await {
        Ok(tx) => tx,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR,
    };
    let deleted = sqlx::query("DELETE FROM sessions WHERE id=? AND EXISTS (SELECT 1 FROM session_runtime r WHERE r.session_id=sessions.id AND (r.status!='running' OR r.lease_until IS NULL OR (julianday(r.lease_until) IS NOT NULL AND julianday(r.lease_until)<julianday(?))))")
        .bind(&id)
        .bind(&now)
        .execute(&mut *tx)
        .await;
    match deleted {
        Ok(r) if r.rows_affected() == 1 => {
            if sqlx::query("DELETE FROM session_runtime WHERE session_id=?")
                .bind(&id)
                .execute(&mut *tx)
                .await
                .is_err()
            {
                let _ = tx.rollback().await;
                return StatusCode::INTERNAL_SERVER_ERROR;
            }
            if tx.commit().await.is_err() {
                return StatusCode::INTERNAL_SERVER_ERROR;
            }
            StatusCode::NO_CONTENT
        }
        Ok(_) => {
            let _ = tx.rollback().await;
            match load_session(&s.db, &id).await {
                Ok(_) => StatusCode::CONFLICT,
                Err(status) => status,
            }
        }
        Err(_) => {
            let _ = tx.rollback().await;
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}
#[derive(Deserialize)]
pub struct SetModel {
    pub model: Option<String>,
}
pub async fn set_session_model(
    Path(id): Path<String>,
    State(s): State<AppState>,
    Json(v): Json<SetModel>,
) -> Result<Json<Session>, StatusCode> {
    load_session(&s.db, &id).await?;
    let model = v
        .model
        .map(|m| m.trim().to_owned())
        .filter(|m| !m.is_empty());
    let result =
        sqlx::query("UPDATE sessions SET model=?,updated_at=? WHERE id=? AND EXISTS (SELECT 1 FROM session_runtime r WHERE r.session_id=sessions.id AND r.status!='running')")
            .bind(&model)
            .bind(Utc::now().to_rfc3339())
            .bind(&id)
            .execute(&s.db)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if result.rows_affected() != 1 {
        return Err(StatusCode::CONFLICT);
    }
    Ok(Json(load_session(&s.db, &id).await?))
}

#[derive(Deserialize)]
pub struct SetThinking {
    pub thinking_level: Option<String>,
}
/// Set the reasoning level passed to the Agent CLI (`--thinking` for Pi).
/// `null`/empty clears it so the Agent's own configured default applies again.
pub async fn set_session_thinking(
    Path(id): Path<String>,
    State(s): State<AppState>,
    Json(v): Json<SetThinking>,
) -> Result<Json<Session>, StatusCode> {
    load_session(&s.db, &id).await?;
    let level = match v.thinking_level {
        Some(raw) if !raw.trim().is_empty() => Some(
            crate::agents::adapter::normalize_thinking_level(&raw)
                .ok_or(StatusCode::BAD_REQUEST)?,
        ),
        _ => None,
    };
    let result =
        sqlx::query("UPDATE sessions SET thinking_level=?,updated_at=? WHERE id=? AND EXISTS (SELECT 1 FROM session_runtime r WHERE r.session_id=sessions.id AND r.status!='running')")
            .bind(&level)
            .bind(Utc::now().to_rfc3339())
            .bind(&id)
            .execute(&s.db)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if result.rows_affected() != 1 {
        return Err(StatusCode::CONFLICT);
    }
    Ok(Json(load_session(&s.db, &id).await?))
}
#[derive(Deserialize)]
pub struct SetToolPreset {
    pub tool_preset: Option<String>,
}
/// Set the tool preset passed to the Pi CLI (`--tools` / `--no-tools`).
/// `null`/empty clears it so Pi's own `defaultTools` applies again.
pub async fn set_session_tools(
    Path(id): Path<String>,
    State(s): State<AppState>,
    Json(v): Json<SetToolPreset>,
) -> Result<Json<Session>, StatusCode> {
    load_session(&s.db, &id).await?;
    let preset = match v.tool_preset.as_deref().map(str::trim) {
        Some(raw) if !raw.is_empty() => Some(
            crate::agents::adapter::normalize_tool_preset(raw).ok_or(StatusCode::BAD_REQUEST)?,
        ),
        _ => None,
    };
    let result =
        sqlx::query("UPDATE sessions SET tool_preset=?,updated_at=? WHERE id=? AND EXISTS (SELECT 1 FROM session_runtime r WHERE r.session_id=sessions.id AND r.status!='running')")
            .bind(&preset)
            .bind(Utc::now().to_rfc3339())
            .bind(&id)
            .execute(&s.db)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if result.rows_affected() != 1 {
        return Err(StatusCode::CONFLICT);
    }
    Ok(Json(load_session(&s.db, &id).await?))
}
#[derive(Deserialize)]
pub struct SetAgent {
    pub agent_id: String,
}
pub async fn set_session_agent(
    Path(id): Path<String>,
    State(s): State<AppState>,
    Json(v): Json<SetAgent>,
) -> Result<Json<Session>, StatusCode> {
    let session = load_session(&s.db, &id).await?;
    let agent_id = v.agent_id.trim();
    if agent_id.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if agent_id == session.agent_id {
        return Ok(Json(session));
    }
    crate::api::runtime::build_agent_config(&s.db, agent_id)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let result=sqlx::query("UPDATE sessions SET agent_id=?,model=NULL,thinking_level=NULL,native_session_id=NULL,fork_from_native_session_id=NULL,updated_at=? WHERE id=? AND EXISTS (SELECT 1 FROM session_runtime r WHERE r.session_id=sessions.id AND r.status!='running')").bind(agent_id).bind(Utc::now().to_rfc3339()).bind(&id).execute(&s.db).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    if result.rows_affected() != 1 {
        return Err(StatusCode::CONFLICT);
    }
    Ok(Json(load_session(&s.db, &id).await?))
}

#[cfg(test)]
mod workspace_path_tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("agentweb-{label}-{}", Uuid::new_v4()))
    }

    #[tokio::test]
    async fn rejects_parent_and_absolute_workspace_paths() {
        let root = temp_root("workspace-path");
        std::fs::create_dir_all(&root).unwrap();

        assert_eq!(
            resolve_workspace_under(&root, "../outside")
                .await
                .unwrap_err(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            resolve_workspace_under(&root, root.join("absolute").to_str().unwrap())
                .await
                .unwrap_err(),
            StatusCode::BAD_REQUEST
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_symlink_escape_before_creating_outside_directory() {
        use std::os::unix::fs::symlink;

        let root = temp_root("workspace-root");
        let outside = temp_root("workspace-outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        symlink(&outside, root.join("escape")).unwrap();

        let result = resolve_workspace_under(&root, "escape/should-not-exist").await;
        assert_eq!(result.unwrap_err(), StatusCode::FORBIDDEN);
        assert!(
            !outside.join("should-not-exist").exists(),
            "a rejected symlink path must not create anything outside the workspace root"
        );

        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::{home_remainder, workspace_like_prefix};

    #[test]
    fn expands_home_prefix_in_requested_paths() {
        assert_eq!(home_remainder("~"), Some(""));
        assert_eq!(home_remainder("~/github/agentweb"), Some("github/agentweb"));
        assert_eq!(home_remainder("$HOME/.dotfiles/"), Some(".dotfiles/"));
        assert_eq!(home_remainder("/home/u/~x"), None);
        assert_eq!(home_remainder("/home/u/github"), None);
    }

    #[test]
    fn escapes_like_wildcards_in_workspace_paths() {
        assert_eq!(workspace_like_prefix("/home/u/plain"), "/home/u/plain/%");
        assert_eq!(
            workspace_like_prefix("/home/u/my_app"),
            "/home/u/my\\_app/%"
        );
        assert_eq!(workspace_like_prefix("/home/u/100%"), "/home/u/100\\%/%");
        assert_eq!(workspace_like_prefix("/home/u/a\\b"), "/home/u/a\\\\b/%");
    }
}
