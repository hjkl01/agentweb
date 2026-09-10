use axum::{extract::{Path as AxumPath, Query, State}, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::path::{Component, PathBuf};
use tokio::fs;
use crate::state::AppState;
use super::sessions::get_session;

#[derive(Deserialize)]
pub struct TreeQuery { pub path: Option<String>, pub search: Option<String> }
#[derive(Serialize)]
pub struct TreeItem { pub name: String, pub path: String, pub kind: &'static str, pub size: u64 }

fn validate_relative(path: &str) -> Result<PathBuf, StatusCode> {
    let path = PathBuf::from(path);
    if path.is_absolute() { return Err(StatusCode::BAD_REQUEST); }
    for component in path.components() {
        if matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_)) { return Err(StatusCode::BAD_REQUEST); }
    }
    Ok(path)
}

fn ignored(name: &str) -> bool { matches!(name, ".git" | "node_modules" | "target") }

async fn search_tree(root: &PathBuf, needle: &str) -> Result<Vec<TreeItem>, StatusCode> {
    let needle = needle.to_lowercase();
    let mut pending = vec![root.clone()];
    let mut result = Vec::new();
    while let Some(dir) = pending.pop() {
        let mut entries = fs::read_dir(&dir).await.map_err(|_| StatusCode::NOT_FOUND)?;
        while let Some(entry) = entries.next_entry().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)? {
            if result.len() >= 2000 { break; }
            let name = entry.file_name().to_string_lossy().into_owned();
            if ignored(&name) { continue; }
            let canonical = match fs::canonicalize(entry.path()).await { Ok(p) => p, Err(_) => continue };
            if !canonical.starts_with(root) { continue; }
            let metadata = match fs::metadata(&canonical).await { Ok(m) => m, Err(_) => continue };
            let relative = match canonical.strip_prefix(root) { Ok(p) => p.to_string_lossy().replace('\\', "/"), Err(_) => continue };
            if relative.to_lowercase().contains(&needle) {
                result.push(TreeItem { name: name.clone(), path: relative, kind: if metadata.is_dir() { "directory" } else { "file" }, size: metadata.len() });
            }
            if metadata.is_dir() { pending.push(canonical); }
        }
        if result.len() >= 2000 { break; }
    }
    result.sort_by(|a,b| a.path.to_lowercase().cmp(&b.path.to_lowercase()));
    Ok(result)
}

pub async fn tree(AxumPath(id): AxumPath<String>, State(state): State<AppState>, Query(query): Query<TreeQuery>) -> Result<Json<Vec<TreeItem>>, StatusCode> {
    let session = get_session(AxumPath(id), State(state)).await.map_err(|_| StatusCode::NOT_FOUND)?.0;
    let root = fs::canonicalize(&session.workspace).await.map_err(|_| StatusCode::NOT_FOUND)?;
    if let Some(search) = query.search.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(Json(search_tree(&root, search).await?));
    }
    let current = match query.path.as_deref() {
        None | Some("") => root.clone(),
        Some(relative) => {
            let relative = validate_relative(relative)?;
            let canonical = fs::canonicalize(root.join(relative)).await.map_err(|_| StatusCode::NOT_FOUND)?;
            if !canonical.starts_with(&root) { return Err(StatusCode::FORBIDDEN); }
            canonical
        }
    };
    let metadata = fs::metadata(&current).await.map_err(|_| StatusCode::NOT_FOUND)?;
    if !metadata.is_dir() { return Err(StatusCode::BAD_REQUEST); }
    let mut entries = fs::read_dir(&current).await.map_err(|_| StatusCode::NOT_FOUND)?;
    let mut result = Vec::new();
    while let Some(entry) = entries.next_entry().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)? {
        let name = entry.file_name().to_string_lossy().into_owned();
        if ignored(&name) { continue; }
        let metadata = match entry.metadata().await { Ok(m) => m, Err(_) => continue };
        let relative = entry.path().strip_prefix(&root).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?.to_string_lossy().replace('\\', "/");
        result.push(TreeItem { name, path: relative, kind: if metadata.is_dir() { "directory" } else { "file" }, size: metadata.len() });
        if result.len() >= 2000 { break; }
    }
    result.sort_by(|a,b| a.kind.cmp(b.kind).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(Json(result))
}
