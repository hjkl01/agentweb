use crate::state::AppState;
use axum::{
    extract::{Path, State},
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
    pub native_session_id: Option<String>,
    pub fork_from_native_session_id: Option<String>,
    pub model: Option<String>,
    pub is_pinned: bool,
    pub workspace_available: bool,
}
fn session_from_row(row: &sqlx::sqlite::SqliteRow) -> Session {
    Session {
        id: row.get(0),
        agent_id: row.get(1),
        title: row.get(2),
        workspace: row.get(3),
        status: row.get(4),
        native_session_id: row.get(5),
        model: row.get(6),
        is_pinned: row.get::<i64, _>(7) != 0,
        fork_from_native_session_id: row.get(8),
        workspace_available: false,
    }
}
fn default_workspace_root() -> PathBuf {
    if let Ok(path) = std::env::var("AGENTWEB_WORKSPACE_DIR") {
        return PathBuf::from(path);
    }
    if let Ok(path) = std::env::var("XDG_DATA_HOME") {
        return PathBuf::from(path).join("agentweb").join("workspaces");
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home)
        .join(".local")
        .join("share")
        .join("agentweb")
        .join("workspaces")
}
pub(crate) async fn load_session_record(db: &sqlx::SqlitePool, id: &str) -> Result<Session, StatusCode> {
    let row=sqlx::query("SELECT id,agent_id,title,workspace,status,native_session_id,model,is_pinned,fork_from_native_session_id FROM sessions WHERE id=?").bind(id).fetch_optional(db).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?.ok_or(StatusCode::NOT_FOUND)?;
    Ok(session_from_row(&row))
}

pub(crate) async fn load_session(db: &sqlx::SqlitePool, id: &str) -> Result<Session, StatusCode> {
    let mut session = load_session_record(db, id).await?;

    // Persisted workspace paths are not trusted: a stale or edited database row
    // must not cause an Agent or terminal to bind an arbitrary host directory.
    let base_path = default_workspace_root();
    fs::create_dir_all(&base_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let base = fs::canonicalize(&base_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let workspace = fs::canonicalize(&session.workspace)
        .await
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::FORBIDDEN
            }
        })?;
    if !workspace.is_dir() || !workspace.starts_with(&base) || workspace == base {
        return Err(StatusCode::FORBIDDEN);
    }
    session.workspace = workspace.to_string_lossy().into_owned();
    session.workspace_available = true;
    Ok(session)
}
pub async fn list_workspaces() -> Result<Json<Vec<String>>, StatusCode> {
    let base_path = default_workspace_root();
    fs::create_dir_all(&base_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let base = fs::canonicalize(&base_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut entries = fs::read_dir(&base)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut workspaces = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        let path = entry.path();
        match fs::metadata(&path).await {
            Ok(metadata) if metadata.is_dir() => {}
            _ => continue,
        }
        let canonical = match fs::canonicalize(&path).await {
            Ok(path) if path.starts_with(&base) => path,
            _ => continue,
        };
        if let Ok(relative) = canonical.strip_prefix(&base) {
            workspaces.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    workspaces.sort();
    Ok(Json(workspaces))
}

pub async fn list_sessions(
    State(s): State<AppState>,
) -> Result<Json<Vec<Session>>, StatusCode> {
    // A failed discovery task is an internal error, not an empty session list:
    // silently treating a worker panic as "no native sessions" hides history.
    let discovered = tokio::task::spawn_blocking(crate::agents::pi_transcript::list_native_sessions)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    // Incomplete discovery may still contribute found sessions, but must never be
    // interpreted as proof that any previously imported native session was deleted.
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
    }

    let rows=sqlx::query("SELECT id,agent_id,title,workspace,status,native_session_id,model,is_pinned,fork_from_native_session_id FROM sessions ORDER BY is_pinned DESC, updated_at DESC")
        .fetch_all(&s.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let base_path = default_workspace_root();
    if fs::create_dir_all(&base_path).await.is_err() {
        return Ok(Json(rows.iter().map(session_from_row).collect()));
    }
    let base = match fs::canonicalize(&base_path).await {
        Ok(path) => path,
        Err(_) => return Ok(Json(rows.iter().map(session_from_row).collect())),
    };
    let mut sessions = Vec::with_capacity(rows.len());
    for row in &rows {
        let mut session = session_from_row(row);
        if let Ok(workspace) = fs::canonicalize(&session.workspace).await {
            session.workspace_available =
                workspace.is_dir() && workspace.starts_with(&base) && workspace != base;
            if session.workspace_available {
                session.workspace = workspace.to_string_lossy().into_owned();
            }
        }
        sessions.push(session);
    }
    Ok(Json(sessions))
}
#[derive(Deserialize)]
pub struct CreateSession {
    pub agent_id: Option<String>,
    pub title: Option<String>,
    pub workspace: Option<String>,
    pub model: Option<String>,
}
async fn resolve_workspace(requested: Option<&str>) -> Result<String, StatusCode> {
    let name = requested.unwrap_or("").trim();
    resolve_workspace_under(&default_workspace_root(), name).await
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
    let id = Uuid::new_v4().to_string();
    let workspace = resolve_workspace(v.workspace.as_deref()).await?;
    let now = Utc::now().to_rfc3339();
    let title = v
        .title
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| "New Chat".into());
    sqlx::query("INSERT INTO sessions(id,agent_id,title,workspace,status,native_session_id,model,is_pinned,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?)").bind(&id).bind(&agent_id).bind(&title).bind(&workspace).bind("idle").bind::<Option<String>>(None).bind(&model).bind(0i64).bind(&now).bind(&now).execute(&s.db).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(Session {
        id,
        agent_id,
        title,
        workspace,
        status: "idle".into(),
        native_session_id: None,
        fork_from_native_session_id: None,
        model,
        is_pinned: false,
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
pub async fn get_session(
    Path(id): Path<String>,
    State(s): State<AppState>,
) -> Result<Json<Session>, StatusCode> {
    let mut session = load_session_record(&s.db, &id).await?;
    let base = fs::canonicalize(default_workspace_root()).await.ok();
    let workspace = fs::canonicalize(&session.workspace).await.ok();
    session.workspace_available = match (base, workspace) {
        (Some(base), Some(workspace)) => {
            let allowed = workspace.is_dir() && workspace.starts_with(&base) && workspace != base;
            if allowed { session.workspace = workspace.to_string_lossy().into_owned(); }
            allowed
        }
        _ => false,
    };
    Ok(Json(session))
}

pub async fn create_pi_fork(
    Path(id): Path<String>,
    State(s): State<AppState>,
) -> Result<Json<Session>, StatusCode> {
    let source = load_session(&s.db, &id).await?;
    if source.status == "running"
        || source.status == "missing"
        || source.native_session_id.is_none()
    {
        return Err(StatusCode::CONFLICT);
    }
    let kind: Option<String> = sqlx::query_scalar("SELECT kind FROM agents WHERE id=?")
        .bind(&source.agent_id)
        .fetch_optional(&s.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if kind.as_deref() != Some("pi") {
        return Err(StatusCode::BAD_REQUEST);
    }
    let native_id = source
        .native_session_id
        .as_deref()
        .ok_or(StatusCode::CONFLICT)?;
    let native_id_owned = native_id.to_owned();
    let exists = tokio::task::spawn_blocking(move || {
        crate::agents::pi_transcript::native_session_file_exists(&native_id_owned)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !exists {
        sqlx::query("UPDATE sessions SET status='missing',updated_at=? WHERE id=? AND native_session_id=? AND status!='running' AND worker_id IS NULL")
            .bind(Utc::now().to_rfc3339())
            .bind(&id)
            .bind(native_id)
            .execute(&s.db)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        return Err(StatusCode::CONFLICT);
    }
    let source_still_available: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE id=? AND native_session_id=? AND status!='running' AND worker_id IS NULL")
        .bind(&id).bind(native_id).fetch_one(&s.db).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if source_still_available != 1 {
        return Err(StatusCode::CONFLICT);
    }

    let new_id = Uuid::new_v4().to_string();
    let title = format!(
        "分支：{}",
        source.title.chars().take(70).collect::<String>()
    );
    let now = Utc::now().to_rfc3339();
    sqlx::query("INSERT INTO sessions(id,agent_id,title,workspace,status,native_session_id,model,is_pinned,created_at,updated_at,fork_from_native_session_id) VALUES(?,?,?,?,?,?,?,?,?,?,?)")
        .bind(&new_id).bind(&source.agent_id).bind(&title).bind(&source.workspace)
        .bind("idle").bind::<Option<String>>(None).bind(&source.model).bind(0i64)
        .bind(&now).bind(&now).bind(native_id).execute(&s.db).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(load_session(&s.db, &new_id).await?))
}

pub async fn restart_missing_session(
    Path(id): Path<String>,
    State(s): State<AppState>,
) -> Result<Json<Session>, StatusCode> {
    let now = Utc::now().to_rfc3339();
    let result = sqlx::query(
        "UPDATE sessions SET native_session_id=NULL,fork_from_native_session_id=NULL,status='idle',worker_id=NULL,lease_until=NULL,updated_at=? WHERE id=? AND status='missing' AND worker_id IS NULL",
    )
    .bind(&now)
    .bind(&id)
    .execute(&s.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if result.rows_affected() != 1 {
        load_session(&s.db, &id).await?;
        return Err(StatusCode::CONFLICT);
    }
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
                "SELECT id,agent_id,title,workspace,status,native_session_id,model,is_pinned,fork_from_native_session_id FROM sessions WHERE id=?",
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
            let result = sqlx::query("DELETE FROM sessions WHERE id=? AND status!='running'")
                .bind(&id)
                .execute(&s.db)
                .await;
            return match result {
                Ok(result) if result.rows_affected() == 1 => StatusCode::NO_CONTENT,
                Ok(_) => StatusCode::CONFLICT,
                Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
            };
        }
        Err(e) => return e,
    };
    if session.status == "running" {
        // Never stop a live execution when deletion will be rejected anyway.
        // Only expired leases may be cleaned up by this endpoint.
        let execution: Option<(Option<String>, Option<String>)> = match sqlx::query_as(
            "SELECT worker_id,lease_until FROM sessions WHERE id=? AND status='running'",
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
        match lease_until.as_deref() {
            Some(lease) => match chrono::DateTime::parse_from_rfc3339(lease) {
                Ok(lease) if lease.with_timezone(&Utc) >= Utc::now() => {
                    return StatusCode::CONFLICT;
                }
                // Treat malformed lease timestamps conservatively: do not stop
                // or delete a possibly live execution based on invalid data.
                Err(_) => return StatusCode::CONFLICT,
                _ => {}
            },
            None => {}
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
    match sqlx::query("DELETE FROM sessions WHERE id=? AND (status!='running' OR lease_until IS NULL OR (julianday(lease_until) IS NOT NULL AND julianday(lease_until)<julianday(?)))")
        .bind(&id)
        .bind(&now)
        .execute(&s.db)
        .await
    {
        Ok(r) if r.rows_affected() == 1 => StatusCode::NO_CONTENT,
        Ok(_) => match load_session(&s.db, &id).await {
            Ok(_) => StatusCode::CONFLICT,
            Err(status) => status,
        },
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
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
        sqlx::query("UPDATE sessions SET model=?,updated_at=? WHERE id=? AND status!='running'")
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
    let result=sqlx::query("UPDATE sessions SET agent_id=?,model=NULL,native_session_id=NULL,fork_from_native_session_id=NULL,updated_at=? WHERE id=? AND status!='running'").bind(agent_id).bind(Utc::now().to_rfc3339()).bind(&id).execute(&s.db).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
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
