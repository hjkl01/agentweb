use super::sessions::get_session;
use crate::state::AppState;
use axum::{extract::{Path, State}, http::StatusCode, Json};
use std::path::{Path as FsPath, PathBuf};
use tokio::{fs, process::Command};

async fn workspace_root(path: &str) -> Result<PathBuf, StatusCode> {
    fs::canonicalize(path).await.map_err(|_| StatusCode::NOT_FOUND)
}

async fn workspace_file_path(workspace: &str, relative: &str) -> Result<PathBuf, StatusCode> {
    if relative.trim().is_empty() || FsPath::new(relative).is_absolute() { return Err(StatusCode::BAD_REQUEST); }
    let root = workspace_root(workspace).await?;
    let candidate = fs::canonicalize(root.join(relative)).await.map_err(|_| StatusCode::NOT_FOUND)?;
    if !candidate.starts_with(&root) { return Err(StatusCode::FORBIDDEN); }
    Ok(candidate)
}

pub async fn workspace_files(Path(id): Path<String>, State(s): State<AppState>) -> Result<Json<Vec<serde_json::Value>>, StatusCode> {
    let session = get_session(Path(id), State(s.clone())).await?.0;
    let root = workspace_root(&session.workspace).await?;
    let mut out = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let mut entries = fs::read_dir(&dir).await.map_err(|_| StatusCode::NOT_FOUND)?;
        while let Some(entry) = entries.next_entry().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)? {
            let path = entry.path();
            let metadata = entry.metadata().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            if metadata.is_dir() { stack.push(path.clone()); }
            let relative = path.strip_prefix(&root).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?.to_string_lossy().replace('\\', "/");
            out.push(serde_json::json!({
                "name": entry.file_name().to_string_lossy(),
                "path": relative,
                "kind": if metadata.is_dir() { "directory" } else { "file" },
                "size": metadata.len()
            }));
        }
    }
    out.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    Ok(Json(out))
}

pub async fn workspace_file(Path((id, path)): Path<(String, String)>, State(s): State<AppState>) -> Result<Json<serde_json::Value>, StatusCode> {
    let session = get_session(Path(id), State(s)).await?.0;
    let file = workspace_file_path(&session.workspace, &path).await?;
    let content = fs::read_to_string(file).await.map_err(|_| StatusCode::NOT_FOUND)?;
    Ok(Json(serde_json::json!({ "path": path, "content": content })))
}

pub async fn workspace_diff(Path(id): Path<String>, State(s): State<AppState>) -> Result<Json<serde_json::Value>, StatusCode> {
    let session = get_session(Path(id), State(s)).await?.0;
    let output = Command::new("git").arg("-C").arg(&session.workspace).args(["diff", "--no-ext-diff"]).output().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    if !output.status.success() { return Err(StatusCode::BAD_GATEWAY); }
    Ok(Json(serde_json::json!({ "status": "ok", "diff": String::from_utf8_lossy(&output.stdout) })))
}
