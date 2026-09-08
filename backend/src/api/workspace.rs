use super::sessions::get_session;
use crate::state::AppState;
use axum::{extract::{Path, State}, http::StatusCode, Json};
use std::path::{Path as FsPath, PathBuf};
use tokio::{fs, process::Command};

const MAX_DIFF_FILE_BYTES: u64 = 1024 * 1024;
const MAX_DIFF_TOTAL_BYTES: usize = 4 * 1024 * 1024;
const MAX_PREVIEW_BYTES: usize = 512 * 1024;
const MAX_WORKSPACE_ENTRIES: usize = 20_000;
const IGNORED_DIRS: &[&str] = &[".git", "node_modules", "target", ".next", "dist", "build", ".cache", ".turbo", "coverage"];

fn ignored_dir(path: &FsPath) -> bool { path.file_name().and_then(|name| name.to_str()).map(|name| IGNORED_DIRS.contains(&name)).unwrap_or(false) }
fn ignored_relative(path: &str) -> bool { path.split('/').any(|part| IGNORED_DIRS.contains(&part)) }
async fn workspace_root(path: &str) -> Result<PathBuf, StatusCode> { fs::canonicalize(path).await.map_err(|_| StatusCode::NOT_FOUND) }

async fn workspace_file_path(workspace: &str, relative: &str) -> Result<PathBuf, StatusCode> {
    if relative.trim().is_empty() || FsPath::new(relative).is_absolute() { return Err(StatusCode::BAD_REQUEST); }
    let root = workspace_root(workspace).await?;
    let candidate = fs::canonicalize(root.join(relative)).await.map_err(|_| StatusCode::NOT_FOUND)?;
    if !candidate.starts_with(&root) { return Err(StatusCode::FORBIDDEN); }
    Ok(candidate)
}

async fn git_status(root: &PathBuf) -> Result<std::collections::HashMap<String, String>, StatusCode> {
    let output = Command::new("git").arg("-C").arg(root).args(["status", "--porcelain=v1", "-z", "--untracked-files=all"]).output().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    if !output.status.success() { return Ok(Default::default()); }
    let mut result = std::collections::HashMap::new();
    let fields = String::from_utf8_lossy(&output.stdout).split('\0').filter(|value| !value.is_empty()).map(str::to_owned).collect::<Vec<_>>();
    let mut index = 0;
    while index < fields.len() {
        let entry = &fields[index];
        if entry.len() < 4 { index += 1; continue; }
        let code = &entry[..2];
        let path = entry[3..].replace('\\', "/");
        let status = if code == "??" { "untracked" } else if code.contains('R') { "renamed" } else if code.contains('D') { "deleted" } else if code.contains('A') { "added" } else { "modified" };
        result.insert(path, status.to_owned());
        if code.contains('R') || code.contains('C') {
            if let Some(previous) = fields.get(index + 1) { if !previous.is_empty() { result.insert(previous.replace('\\', "/"), "deleted".to_owned()); } index += 1; }
        }
        index += 1;
    }
    Ok(result)
}

pub async fn workspace_files(Path(id): Path<String>, State(s): State<AppState>) -> Result<Json<Vec<serde_json::Value>>, StatusCode> {
    let session = get_session(Path(id), State(s.clone())).await?.0;
    let root = workspace_root(&session.workspace).await?;
    let statuses = git_status(&root).await.unwrap_or_default();
    let mut out = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let mut entries = fs::read_dir(&dir).await.map_err(|_| StatusCode::NOT_FOUND)?;
        while let Some(entry) = entries.next_entry().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)? {
            if out.len() >= MAX_WORKSPACE_ENTRIES { break; }
            let path = entry.path();
            let metadata = entry.metadata().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            if metadata.is_dir() { if ignored_dir(&path) { continue; } stack.push(path.clone()); }
            let relative = path.strip_prefix(&root).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?.to_string_lossy().replace('\\', "/");
            out.push(serde_json::json!({ "name": entry.file_name().to_string_lossy(), "path": relative, "kind": if metadata.is_dir() { "directory" } else { "file" }, "size": metadata.len(), "status": statuses.get(&relative) }));
        }
        if out.len() >= MAX_WORKSPACE_ENTRIES { break; }
    }
    for (path, status) in &statuses { if status == "deleted" && !out.iter().any(|item| item["path"].as_str() == Some(path)) { out.push(serde_json::json!({ "name": path.rsplit('/').next().unwrap_or(path), "path": path, "kind": "file", "size": 0, "status": status })); } }
    out.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    Ok(Json(out))
}

async fn git_file(root: &PathBuf, path: &str) -> Result<(String, usize), StatusCode> {
    if ignored_relative(path) { return Err(StatusCode::FORBIDDEN); }
    let output = Command::new("git").arg("-C").arg(root).args(["show", &format!("HEAD:{path}")]).output().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    if !output.status.success() { return Err(StatusCode::NOT_FOUND); }
    if output.stdout.len() > MAX_PREVIEW_BYTES { return Err(StatusCode::PAYLOAD_TOO_LARGE); }
    if output.stdout.contains(&0) { return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE); }
    Ok((String::from_utf8_lossy(&output.stdout).into_owned(), output.stdout.len()))
}

pub async fn workspace_file(Path((id, path)): Path<(String, String)>, State(s): State<AppState>) -> Result<Json<serde_json::Value>, StatusCode> {
    let session = get_session(Path(id), State(s)).await?.0;
    let file = match workspace_file_path(&session.workspace, &path).await {
        Ok(file) => file,
        Err(StatusCode::NOT_FOUND) => {
            let root = workspace_root(&session.workspace).await?;
            let (content, size) = git_file(&root, &path).await?;
            return Ok(Json(serde_json::json!({ "path": path, "content": content, "size": size, "truncated": false, "binary": false, "source": "git" })));
        }
        Err(status) => return Err(status),
    };
    let metadata = fs::metadata(&file).await.map_err(|_| StatusCode::NOT_FOUND)?;
    if metadata.is_dir() { return Err(StatusCode::BAD_REQUEST); }
    if metadata.len() > MAX_PREVIEW_BYTES as u64 { return Ok(Json(serde_json::json!({ "path": path, "content": "", "size": metadata.len(), "truncated": true, "binary": false, "message": "File is too large to preview (limit: 512 KiB)." }))); }
    let bytes = fs::read(&file).await.map_err(|_| StatusCode::NOT_FOUND)?;
    if bytes.contains(&0) { return Ok(Json(serde_json::json!({ "path": path, "content": "", "size": bytes.len(), "truncated": false, "binary": true, "message": "Binary file preview is not supported." }))); }
    Ok(Json(serde_json::json!({ "path": path, "content": String::from_utf8_lossy(&bytes).into_owned(), "size": bytes.len(), "truncated": false, "binary": false, "source": "workspace" })))
}

fn append_diff(diff: &mut String, block: &str) -> bool {
    let remaining = MAX_DIFF_TOTAL_BYTES.saturating_sub(diff.len());
    if block.len() <= remaining { diff.push_str(block); return false; }
    if remaining == 0 { return true; }
    let mut end = remaining.min(block.len());
    while end > 0 && !block.is_char_boundary(end) { end -= 1; }
    if let Some(newline) = block[..end].rfind('\n') { end = newline + 1; }
    diff.push_str(&block[..end]);
    true
}

pub async fn workspace_diff(Path(id): Path<String>, State(s): State<AppState>) -> Result<Json<serde_json::Value>, StatusCode> {
    let session = get_session(Path(id), State(s)).await?.0;
    let root = workspace_root(&session.workspace).await?;
    let output = Command::new("git").arg("-C").arg(&root).args(["diff", "HEAD", "--no-ext-diff", "--binary"]).output().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    if !output.status.success() { return Err(StatusCode::BAD_GATEWAY); }
    let mut diff = String::from_utf8_lossy(&output.stdout).into_owned();
    let mut truncated = false;
    if diff.len() > MAX_DIFF_TOTAL_BYTES { let cut = diff[..MAX_DIFF_TOTAL_BYTES].rfind('\n').map(|n| n + 1).unwrap_or(0); diff.truncate(cut); truncated = true; }
    if !truncated {
        let untracked = Command::new("git").arg("-C").arg(&root).args(["ls-files", "--others", "--exclude-standard", "-z"]).output().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
        if untracked.status.success() {
            for entry in untracked.stdout.split(|byte| *byte == 0).filter(|entry| !entry.is_empty()) {
                let relative = String::from_utf8_lossy(entry).into_owned();
                if ignored_relative(&relative) { continue; }
                let path = root.join(&relative);
                let metadata = match fs::metadata(&path).await { Ok(value) => value, Err(_) => continue };
                if !metadata.is_file() || metadata.len() > MAX_DIFF_FILE_BYTES { continue; }
                let bytes = match fs::read(&path).await { Ok(value) => value, Err(_) => continue };
                if bytes.contains(&0) { continue; }
                let content = String::from_utf8_lossy(&bytes);
                let lines = content.lines().map(|line| format!("+{line}")).collect::<Vec<_>>();
                let block = format!("diff --git a/{relative} b/{relative}\nnew file mode 100644\n--- /dev/null\n+++ b/{relative}\n@@ -0,0 +1,{} @@\n{}\n", lines.len(), lines.join("\n"));
                if append_diff(&mut diff, &block) { truncated = true; break; }
            }
        }
    }
    Ok(Json(serde_json::json!({ "status": "ok", "diff": diff, "truncated": truncated })))
}
