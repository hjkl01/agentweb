use axum::http::StatusCode;
use std::path::{Path, PathBuf};
use tokio::fs;

pub async fn resolve_workspace(input: &str) -> Result<PathBuf, StatusCode> {
    let base_path = std::env::var("AGENTWEB_WORKSPACE_DIR")
        .unwrap_or_else(|_| "./workspaces".to_owned());
    fs::create_dir_all(&base_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let base = fs::canonicalize(&base_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let raw = input.trim();
    let candidate = if raw.is_empty() || raw == "." {
        base.clone()
    } else {
        let path = Path::new(raw);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            base.join(path)
        }
    };

    fs::create_dir_all(&candidate)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let candidate = fs::canonicalize(&candidate)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    if !candidate.starts_with(&base) {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(candidate)
}

pub async fn workspace_root(path: &str) -> Result<PathBuf, StatusCode> {
    let root = fs::canonicalize(path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let base_path = std::env::var("AGENTWEB_WORKSPACE_DIR")
        .unwrap_or_else(|_| "./workspaces".to_owned());
    let base = fs::canonicalize(base_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !root.starts_with(&base) {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(root)
}
