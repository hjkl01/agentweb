use super::sessions::get_session;
use crate::state::AppState;
use axum::{
    body::{Body, Bytes},
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::Response,
    Json,
};
use std::{
    path::{Component, Path as FsPath, PathBuf},
    process::Stdio,
};
use tokio::{fs, io::AsyncReadExt, process::Command};

const MAX_DIFF_FILE_BYTES: u64 = 1024 * 1024;
const MAX_DIFF_TOTAL_BYTES: usize = 4 * 1024 * 1024;
const MAX_PREVIEW_BYTES: usize = 512 * 1024;
const IGNORED_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    ".next",
    "dist",
    "build",
    ".cache",
    ".turbo",
    "coverage",
];

fn ignored_relative(path: &str) -> bool {
    path.split('/').any(|part| IGNORED_DIRS.contains(&part))
}
fn validate_relative(path: &str) -> Result<(), StatusCode> {
    let path = FsPath::new(path);
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(())
}
async fn workspace_root(path: &str) -> Result<PathBuf, StatusCode> {
    fs::canonicalize(path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)
}
async fn workspace_file_path(workspace: &str, relative: &str) -> Result<PathBuf, StatusCode> {
    validate_relative(relative)?;
    let root = workspace_root(workspace).await?;
    let mut current = root.clone();

    // Reject symlinks in every path component, not just the final file. A
    // directory symlink in the middle of a path can otherwise redirect a
    // preview to another location before the final canonical-path check.
    for component in FsPath::new(relative).components() {
        if let Component::Normal(name) = component {
            current.push(name);
            let metadata = fs::symlink_metadata(&current)
                .await
                .map_err(|_| StatusCode::NOT_FOUND)?;
            if metadata.file_type().is_symlink() {
                return Err(StatusCode::FORBIDDEN);
            }
        }
    }

    let candidate = fs::canonicalize(&current)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    if !candidate.starts_with(&root) {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(candidate)
}
async fn read_untracked_file(root: &FsPath, relative: &str) -> Option<Vec<u8>> {
    if ignored_relative(relative) {
        return None;
    }
    let path = root.join(relative);
    // Reject symlinks entirely so diffs represent the link as a filesystem
    // entry rather than reading its target, and cannot disclose host files.
    let link_metadata = fs::symlink_metadata(&path).await.ok()?;
    if link_metadata.file_type().is_symlink() {
        return None;
    }
    let canonical = fs::canonicalize(&path).await.ok()?;
    if !canonical.starts_with(root) {
        return None;
    }
    let metadata = fs::metadata(&canonical).await.ok()?;
    if !metadata.is_file() || metadata.len() > MAX_DIFF_FILE_BYTES {
        return None;
    }
    // File size can change after metadata is checked. Bound the actual read
    // too, so concurrent writes cannot make this endpoint buffer an oversized
    // untracked file into memory.
    let file = fs::File::open(canonical).await.ok()?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_DIFF_FILE_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .await
        .ok()?;
    if bytes.len() as u64 > MAX_DIFF_FILE_BYTES {
        return None;
    }
    Some(bytes)
}

struct LimitedGitOutput {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    truncated: bool,
}

async fn run_git_limited(
    root: &FsPath,
    args: &[&str],
    max_bytes: usize,
) -> Result<LimitedGitOutput, StatusCode> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| {
            tracing::error!(workspace = %root.display(), %error, "failed to start git command");
            StatusCode::BAD_GATEWAY
        })?;

    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill().await;
        let _ = child.wait().await;
        return Err(StatusCode::BAD_GATEWAY);
    };
    let mut bytes = Vec::with_capacity(max_bytes.min(64 * 1024));
    let read_limit = max_bytes.saturating_add(1) as u64;
    if stdout
        .take(read_limit)
        .read_to_end(&mut bytes)
        .await
        .is_err()
    {
        // Always reap the child when reading its pipe fails. Otherwise an
        // unexpected I/O error can leave a Git process running in the
        // background after the request has already failed.
        let _ = child.kill().await;
        let _ = child.wait().await;
        return Err(StatusCode::BAD_GATEWAY);
    }

    let truncated = bytes.len() > max_bytes;
    if truncated {
        // Stop the process once the output limit is exceeded instead of
        // buffering an arbitrarily large diff in memory.
        let _ = child.kill().await;
    }
    let status = child.wait().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    if truncated {
        bytes.truncate(max_bytes);
    }
    Ok(LimitedGitOutput {
        status,
        stdout: bytes,
        truncated,
    })
}

async fn git_status(
    root: &PathBuf,
) -> Result<std::collections::HashMap<String, String>, StatusCode> {
    let output = run_git_limited(
        root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        MAX_DIFF_TOTAL_BYTES,
    )
    .await?;
    if !output.status.success() || output.truncated {
        // A partial porcelain stream may end halfway through a path; don't
        // expose misleading status entries when the safety cap is reached.
        return Ok(Default::default());
    }
    let mut result = std::collections::HashMap::new();
    let fields = String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut index = 0;
    while index < fields.len() {
        let entry = &fields[index];
        if entry.len() < 4 {
            index += 1;
            continue;
        }
        let code = &entry[..2];
        let path = entry[3..].replace('\\', "/");
        let status = if code == "??" {
            "untracked"
        } else if code.contains('R') {
            "renamed"
        } else if code.contains('D') {
            "deleted"
        } else if code.contains('A') {
            "added"
        } else {
            "modified"
        };
        result.insert(path, status.to_owned());
        if code.contains('R') || code.contains('C') {
            if let Some(previous) = fields.get(index + 1) {
                if !previous.is_empty() {
                    result.insert(previous.replace('\\', "/"), "deleted".to_owned());
                }
                index += 1;
            }
        }
        index += 1;
    }
    Ok(result)
}

#[derive(serde::Deserialize)]
pub struct WorkspaceFilesQuery {
    /// Relative directory path; omitted or empty means the workspace root.
    pub path: Option<String>,
}

pub async fn workspace_files(
    Path(id): Path<String>,
    State(s): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<WorkspaceFilesQuery>,
) -> Result<Json<Vec<serde_json::Value>>, StatusCode> {
    let session = get_session(Path(id), State(s.clone())).await?.0;
    let root = workspace_root(&session.workspace).await?;
    let current = match query.path.as_deref().map(str::trim) {
        None | Some("") | Some(".") => root.clone(),
        Some(relative) => {
            if ignored_relative(relative) {
                return Err(StatusCode::FORBIDDEN);
            }
            workspace_file_path(&session.workspace, relative).await?
        }
    };
    let current_metadata = fs::metadata(&current)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    if !current_metadata.is_dir() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let statuses = git_status(&root).await.unwrap_or_default();
    let current_relative = current
        .strip_prefix(&root)
        .map_err(|_| StatusCode::FORBIDDEN)?
        .to_string_lossy()
        .replace('\\', "/");
    let mut out = Vec::new();
    let mut entries = fs::read_dir(&current)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        if out.len() >= 2000 {
            break;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if IGNORED_DIRS.contains(&name.as_str()) {
            continue;
        }
        let file_type = entry
            .file_type()
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if file_type.is_symlink() {
            continue;
        }
        let metadata = entry
            .metadata()
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let relative = path
            .strip_prefix(&root)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .to_string_lossy()
            .replace('\\', "/");
        out.push(serde_json::json!({
            "name": name,
            "path": relative,
            "kind": if metadata.is_dir() { "directory" } else { "file" },
            "size": metadata.len(),
            "status": statuses.get(&relative)
        }));
    }

    // Keep deleted Git entries visible, but only for the directory currently
    // being browsed; do not return descendants from unopened folders.
    for (path, status) in &statuses {
        if out.len() >= 2000 {
            break;
        }
        let parent = path
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .unwrap_or("");
        if status == "deleted"
            && parent == current_relative
            && !out.iter().any(|item| item["path"].as_str() == Some(path))
        {
            out.push(serde_json::json!({
                "name": path.rsplit('/').next().unwrap_or(path),
                "path": path,
                "kind": "file",
                "size": 0,
                "status": status
            }));
        }
    }
    out.sort_by(|a, b| {
        (a["kind"].as_str() != Some("directory"))
            .cmp(&(b["kind"].as_str() != Some("directory")))
            .then_with(|| {
                a["name"]
                    .as_str()
                    .unwrap_or("")
                    .to_lowercase()
                    .cmp(&b["name"].as_str().unwrap_or("").to_lowercase())
            })
    });
    Ok(Json(out))
}

async fn git_file(root: &PathBuf, path: &str) -> Result<(String, usize), StatusCode> {
    validate_relative(path)?;
    if ignored_relative(path) {
        return Err(StatusCode::FORBIDDEN);
    }
    let spec = format!("HEAD:{path}");
    let output = run_git_limited(root, &["show", &spec], MAX_PREVIEW_BYTES).await?;
    if output.truncated {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    if !output.status.success() {
        return Err(StatusCode::NOT_FOUND);
    }
    if output.stdout.contains(&0) {
        return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
    Ok((
        String::from_utf8_lossy(&output.stdout).into_owned(),
        output.stdout.len(),
    ))
}

pub async fn workspace_file(
    Path((id, path)): Path<(String, String)>,
    State(s): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let session = get_session(Path(id), State(s)).await?.0;
    let file = match workspace_file_path(&session.workspace, &path).await {
        Ok(file) => file,
        Err(StatusCode::NOT_FOUND) => {
            let root = workspace_root(&session.workspace).await?;
            let (content, size) = git_file(&root, &path).await?;
            return Ok(Json(
                serde_json::json!({ "path": path, "content": content, "size": size, "truncated": false, "binary": false, "source": "git" }),
            ));
        }
        Err(status) => return Err(status),
    };
    let metadata = fs::metadata(&file)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    if metadata.is_dir() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if metadata.len() > MAX_PREVIEW_BYTES as u64 {
        return Ok(Json(
            serde_json::json!({ "path": path, "content": "", "size": metadata.len(), "truncated": true, "binary": false, "message": "File is too large to preview (limit: 512 KiB)." }),
        ));
    }
    // Bound the actual read as well as the metadata check: the file may
    // grow between stat and read.
    let opened = fs::File::open(&file)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    opened
        .take((MAX_PREVIEW_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    if bytes.len() > MAX_PREVIEW_BYTES {
        return Ok(Json(
            serde_json::json!({ "path": path, "content": "", "size": bytes.len(), "truncated": true, "binary": false, "message": "File is too large to preview (limit: 512 KiB)." }),
        ));
    }
    if bytes.contains(&0) {
        return Ok(Json(
            serde_json::json!({ "path": path, "content": "", "size": bytes.len(), "truncated": false, "binary": true, "message": "Binary file preview is not supported." }),
        ));
    }
    Ok(Json(
        serde_json::json!({ "path": path, "content": String::from_utf8_lossy(&bytes).into_owned(), "size": bytes.len(), "truncated": false, "binary": false, "source": "workspace" }),
    ))
}

/// Largest inline-preview payload. Images/audio/PDF are streamed whole, so this
/// is the boundary that keeps a huge asset from exhausting memory. Bump it if
/// large PDFs become a real use case.
/// ponytail: read fully into memory; stream/chunk if large files matter.
const MAX_RAW_BYTES: u64 = 25 * 1024 * 1024;

/// Allow-list of previewable binary types. Anything not listed returns 415;
/// this is the security boundary for what the browser may render inline.
fn raw_content_type(path: &str) -> Option<&'static str> {
    let ext = FsPath::new(path)
        .extension()
        .and_then(|value| value.to_str())?
        .to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "svg" => "image/svg+xml",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" | "oga" => "audio/ogg",
        "m4a" => "audio/mp4",
        "flac" => "audio/flac",
        "pdf" => "application/pdf",
        _ => return None,
    })
}

/// Stream a workspace file with its real content type for inline preview
/// (`<img>`, `<audio>`, `<iframe>`). Mounted behind the same cookie auth and
/// path/symlink validation as the JSON preview endpoint.
pub async fn workspace_raw(
    Path((id, path)): Path<(String, String)>,
    State(s): State<AppState>,
) -> Result<Response, StatusCode> {
    let Some(content_type) = raw_content_type(&path) else {
        return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    };
    let session = get_session(Path(id), State(s)).await?.0;
    let file = workspace_file_path(&session.workspace, &path).await?;
    let metadata = fs::metadata(&file)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    if !metadata.is_file() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if metadata.len() > MAX_RAW_BYTES {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    let bytes = fs::read(&file).await.map_err(|_| StatusCode::NOT_FOUND)?;
    if bytes.len() as u64 > MAX_RAW_BYTES {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_DISPOSITION, "inline")
        .header("x-content-type-options", "nosniff")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(bytes))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

const MAX_UPLOAD_BYTES: usize = 25 * 1024 * 1024;

#[derive(serde::Deserialize)]
pub struct UploadQuery {
    pub dir: Option<String>,
    pub name: String,
}

#[derive(serde::Serialize)]
pub struct UploadedFile {
    pub path: String,
    pub size: usize,
}

/// Accept exactly one normal path component: reject separators, `.`, `..`,
/// absolute paths, and empty names so an upload cannot pick its own directory.
fn upload_file_name(name: &str) -> Result<&str, StatusCode> {
    let name = name.trim();
    let mut components = FsPath::new(name).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) if name != "." && name != ".." => Ok(name),
        _ => Err(StatusCode::BAD_REQUEST),
    }
}

/// `POST /sessions/{id}/upload?name=file.png[&dir=sub]`
///
/// Writes the raw request body into the session workspace. The destination is
/// staged in the same directory and renamed into place, so a failed or
/// interrupted write never leaves a partial file. An existing destination is
/// refused (409) rather than overwritten.
pub async fn workspace_upload(
    Path(id): Path<String>,
    Query(query): Query<UploadQuery>,
    State(s): State<AppState>,
    body: Bytes,
) -> Result<Json<UploadedFile>, StatusCode> {
    let name = upload_file_name(&query.name)?.to_owned();
    if body.len() > MAX_UPLOAD_BYTES {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    let session = get_session(Path(id), State(s)).await?.0;
    let root = workspace_root(&session.workspace).await?;
    let dir = query.dir.as_deref().unwrap_or("").trim().trim_matches('/');
    let parent = if dir.is_empty() || dir == "." {
        root
    } else {
        let path = workspace_file_path(&session.workspace, dir).await?;
        if !fs::metadata(&path)
            .await
            .map_err(|_| StatusCode::NOT_FOUND)?
            .is_dir()
        {
            return Err(StatusCode::BAD_REQUEST);
        }
        path
    };
    let destination = parent.join(&name);
    // Existing file, directory, or symlink: refuse rather than clobber.
    if fs::symlink_metadata(&destination).await.is_ok() {
        return Err(StatusCode::CONFLICT);
    }
    let temp = parent.join(format!(".{name}.upload-{}", uuid::Uuid::new_v4()));
    if fs::write(&temp, &body).await.is_err() || fs::rename(&temp, &destination).await.is_err() {
        let _ = fs::remove_file(&temp).await;
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    let path = if dir.is_empty() || dir == "." {
        name
    } else {
        format!("{dir}/{name}")
    };
    Ok(Json(UploadedFile {
        path,
        size: body.len(),
    }))
}

fn append_diff(diff: &mut String, block: &str) -> bool {
    let remaining = MAX_DIFF_TOTAL_BYTES.saturating_sub(diff.len());
    if block.len() <= remaining {
        diff.push_str(block);
        return false;
    }
    if remaining == 0 {
        return true;
    }
    let mut end = remaining.min(block.len());
    while end > 0 && !block.is_char_boundary(end) {
        end -= 1;
    }
    if let Some(newline) = block[..end].rfind('\n') {
        end = newline + 1;
    }
    diff.push_str(&block[..end]);
    true
}

pub async fn workspace_diff(
    Path(id): Path<String>,
    State(s): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let session = get_session(Path(id), State(s)).await?.0;
    let root = workspace_root(&session.workspace).await?;
    let output = run_git_limited(
        &root,
        &["diff", "HEAD", "--no-ext-diff", "--binary"],
        MAX_DIFF_TOTAL_BYTES,
    )
    .await?;
    if output.truncated {
        let diff = String::from_utf8_lossy(&output.stdout).into_owned();
        return Ok(Json(serde_json::json!({
            "status": "ok",
            "diff": diff,
            "truncated": true
        })));
    }
    let mut truncated = false;
    let mut diff = if output.status.success() {
        String::from_utf8_lossy(&output.stdout).into_owned()
    } else {
        // New workspaces may not be Git repositories, and a newly initialized
        // repository may not have a HEAD commit yet. Neither should break the
        // conversation's diff panel.
        let repository = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["rev-parse", "--is-inside-work-tree"])
            .output()
            .await
            .map_err(|error| {
                tracing::error!(workspace = %root.display(), %error, "failed to inspect workspace Git status");
                StatusCode::BAD_GATEWAY
            })?;
        if !repository.status.success() {
            tracing::debug!(workspace = %root.display(), "workspace is not a Git repository; returning an empty diff");
            return Ok(Json(serde_json::json!({
                "status": "ok",
                "diff": "",
                "truncated": false
            })));
        }

        // A Git repository without a first commit has no resolvable HEAD.
        // Show staged and unstaged changes separately as a best-effort diff.
        let cached = run_git_limited(
            &root,
            &["diff", "--cached", "--no-ext-diff", "--binary"],
            MAX_DIFF_TOTAL_BYTES,
        )
        .await?;
        if cached.truncated {
            return Ok(Json(serde_json::json!({
                "status": "ok",
                "diff": String::from_utf8_lossy(&cached.stdout).into_owned(),
                "truncated": true
            })));
        }
        let unstaged = run_git_limited(
            &root,
            &["diff", "--no-ext-diff", "--binary"],
            MAX_DIFF_TOTAL_BYTES.saturating_sub(cached.stdout.len()),
        )
        .await?;
        if !cached.status.success() || !unstaged.status.success() {
            tracing::warn!(
                workspace = %root.display(),
                cached_status = %cached.status,
                unstaged_status = %unstaged.status,
                "git diff HEAD failed and fallback diff commands also failed"
            );
            return Err(StatusCode::BAD_GATEWAY);
        }
        let mut fallback = String::from_utf8_lossy(&cached.stdout).into_owned();
        fallback.push_str(&String::from_utf8_lossy(&unstaged.stdout));
        truncated = unstaged.truncated;
        fallback
    };
    if diff.len() > MAX_DIFF_TOTAL_BYTES {
        // The byte limit may land inside a multi-byte UTF-8 character.
        // Find a valid boundary before searching for the last complete line.
        let mut end = MAX_DIFF_TOTAL_BYTES;
        while end > 0 && !diff.is_char_boundary(end) {
            end -= 1;
        }
        let cut = diff[..end].rfind('\n').map(|n| n + 1).unwrap_or(0);
        diff.truncate(cut);
        truncated = true;
    }
    if !truncated {
        let untracked = run_git_limited(
            &root,
            &["ls-files", "--others", "--exclude-standard", "-z"],
            MAX_DIFF_TOTAL_BYTES,
        )
        .await?;
        if untracked.truncated {
            return Ok(Json(serde_json::json!({
                "status": "ok",
                "diff": diff,
                "truncated": true
            })));
        }
        if untracked.status.success() {
            for entry in untracked
                .stdout
                .split(|byte| *byte == 0)
                .filter(|entry| !entry.is_empty())
            {
                let relative = String::from_utf8_lossy(entry).into_owned();
                // Reject symlink escapes before reading untracked file content.
                let bytes = match read_untracked_file(&root, &relative).await {
                    Some(value) => value,
                    None => continue,
                };
                if bytes.contains(&0) {
                    continue;
                }
                let content = String::from_utf8_lossy(&bytes);
                let lines = content
                    .lines()
                    .map(|line| format!("+{line}"))
                    .collect::<Vec<_>>();
                let block = format!("diff --git a/{relative} b/{relative}\nnew file mode 100644\n--- /dev/null\n+++ b/{relative}\n@@ -0,0 +1,{} @@\n{}\n", lines.len(), lines.join("\n"));
                if append_diff(&mut diff, &block) {
                    truncated = true;
                    break;
                }
            }
        }
    }
    Ok(Json(
        serde_json::json!({ "status": "ok", "diff": diff, "truncated": truncated }),
    ))
}

#[cfg(test)]
mod workspace_file_safety_tests {
    use super::*;
    use uuid::Uuid;

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("agentweb-{label}-{}", Uuid::new_v4()))
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn untracked_diff_does_not_read_symlink_outside_workspace() {
        use std::os::unix::fs::symlink;

        let root = temp_root("diff-root");
        let outside = temp_root("diff-outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), "must-not-leak").unwrap();
        symlink(outside.join("secret.txt"), root.join("secret-link.txt")).unwrap();

        assert!(read_untracked_file(&root, "secret-link.txt")
            .await
            .is_none());

        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn workspace_preview_rejects_symlink_components() {
        use std::os::unix::fs::symlink;

        let root = temp_root("preview-root");
        let outside = temp_root("preview-outside");
        std::fs::create_dir_all(root.join("nested")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), "must-not-leak").unwrap();
        symlink(&outside, root.join("linked-dir")).unwrap();
        symlink(outside.join("secret.txt"), root.join("linked-file.txt")).unwrap();

        assert_eq!(
            workspace_file_path(root.to_str().unwrap(), "linked-file.txt").await,
            Err(StatusCode::FORBIDDEN)
        );
        assert_eq!(
            workspace_file_path(root.to_str().unwrap(), "linked-dir/secret.txt").await,
            Err(StatusCode::FORBIDDEN)
        );

        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn append_diff_truncates_only_at_utf8_boundaries() {
        let mut diff = String::new();
        let block = format!("{}你\n", "x".repeat(MAX_DIFF_TOTAL_BYTES - 2));
        let truncated = append_diff(&mut diff, &block);
        assert!(truncated);
        assert!(diff.is_char_boundary(diff.len()));
        assert!(diff.len() <= MAX_DIFF_TOTAL_BYTES);
    }

    #[tokio::test]
    async fn git_command_output_is_bounded() {
        let root = temp_root("git-output-limit");
        std::fs::create_dir_all(&root).unwrap();

        let output = run_git_limited(&root, &["--version"], 1).await.unwrap();
        assert!(output.truncated);
        assert_eq!(output.stdout.len(), 1);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn untracked_diff_skips_files_that_grow_past_the_limit() {
        let root = temp_root("diff-large-file");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("large.txt"),
            vec![b'x'; MAX_DIFF_FILE_BYTES as usize + 1],
        )
        .unwrap();

        assert!(read_untracked_file(&root, "large.txt").await.is_none());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn untracked_diff_reads_regular_file_inside_workspace() {
        let root = temp_root("diff-regular");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("hello.txt"), "hello").unwrap();

        assert_eq!(
            read_untracked_file(&root, "hello.txt").await.unwrap(),
            b"hello"
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn raw_preview_only_allows_listed_types() {
        assert_eq!(raw_content_type("a/b/pic.PNG"), Some("image/png"));
        assert_eq!(raw_content_type("clip.mp3"), Some("audio/mpeg"));
        assert_eq!(raw_content_type("doc.pdf"), Some("application/pdf"));
        assert_eq!(raw_content_type("archive.zip"), None);
        assert_eq!(raw_content_type("script.html"), None);
        assert_eq!(raw_content_type("noext"), None);
    }

    #[test]
    fn upload_name_must_be_a_single_normal_component() {
        assert_eq!(upload_file_name("  shot.png ").unwrap(), "shot.png");
        assert_eq!(upload_file_name(".env").unwrap(), ".env");
        for rejected in ["", "  ", ".", "..", "a/b", "../escape", "/etc/passwd"] {
            assert_eq!(
                upload_file_name(rejected).unwrap_err(),
                StatusCode::BAD_REQUEST,
                "{rejected:?} must be rejected"
            );
        }
    }
}
