use super::sessions::get_session;
use crate::state::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use std::path::{Component, Path as FsPath, PathBuf};
use tokio::{fs, io::AsyncReadExt, process::{Command, Stdio}};

const MAX_DIFF_FILE_BYTES: u64 = 1024 * 1024;
const MAX_DIFF_TOTAL_BYTES: usize = 4 * 1024 * 1024;
const MAX_PREVIEW_BYTES: usize = 512 * 1024;
const MAX_WORKSPACE_ENTRIES: usize = 20_000;
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

fn ignored_dir(path: &FsPath) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| IGNORED_DIRS.contains(&name))
        .unwrap_or(false)
}
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
    fs::read(canonical).await.ok()
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

    let mut stdout = child.stdout.take().ok_or(StatusCode::BAD_GATEWAY)?;
    let mut bytes = Vec::with_capacity(max_bytes.min(64 * 1024));
    let read_limit = max_bytes.saturating_add(1) as u64;
    stdout
        .take(read_limit)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

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
    Ok(LimitedGitOutput { status, stdout: bytes, truncated })
}

async fn git_status(
    root: &PathBuf,
) -> Result<std::collections::HashMap<String, String>, StatusCode> {
    let output = run_git_limited(
        root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        MAX_DIFF_TOTAL_BYTES,
    ).await?;
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

pub async fn workspace_files(
    Path(id): Path<String>,
    State(s): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, StatusCode> {
    let session = get_session(Path(id), State(s.clone())).await?.0;
    let root = workspace_root(&session.workspace).await?;
    let statuses = git_status(&root).await.unwrap_or_default();
    let mut out = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let mut entries = fs::read_dir(&dir)
            .await
            .map_err(|_| StatusCode::NOT_FOUND)?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        {
            if out.len() >= MAX_WORKSPACE_ENTRIES {
                break;
            }
            let path = entry.path();
            let file_type = entry
                .file_type()
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            // Keep symbolic links out of recursive traversal. A link may point
            // outside the workspace or back to an ancestor and create a cycle.
            if file_type.is_symlink() {
                continue;
            }
            let metadata = entry
                .metadata()
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            if metadata.is_dir() {
                if ignored_dir(&path) {
                    continue;
                }
                stack.push(path.clone());
            }
            let relative = path
                .strip_prefix(&root)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                .to_string_lossy()
                .replace('\\', "/");
            out.push(serde_json::json!({ "name": entry.file_name().to_string_lossy(), "path": relative, "kind": if metadata.is_dir() { "directory" } else { "file" }, "size": metadata.len(), "status": statuses.get(&relative) }));
        }
        if out.len() >= MAX_WORKSPACE_ENTRIES {
            break;
        }
    }
    for (path, status) in &statuses {
        if status == "deleted" && !out.iter().any(|item| item["path"].as_str() == Some(path)) {
            out.push(serde_json::json!({ "name": path.rsplit('/').next().unwrap_or(path), "path": path, "kind": "file", "size": 0, "status": status }));
        }
    }
    out.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
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
    let bytes = fs::read(&file).await.map_err(|_| StatusCode::NOT_FOUND)?;
    if bytes.contains(&0) {
        return Ok(Json(
            serde_json::json!({ "path": path, "content": "", "size": bytes.len(), "truncated": false, "binary": true, "message": "Binary file preview is not supported." }),
        ));
    }
    Ok(Json(
        serde_json::json!({ "path": path, "content": String::from_utf8_lossy(&bytes).into_owned(), "size": bytes.len(), "truncated": false, "binary": false, "source": "workspace" }),
    ))
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
    ).await?;
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
        ).await?;
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
        ).await?;
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
        let cut = diff[..MAX_DIFF_TOTAL_BYTES]
            .rfind('\n')
            .map(|n| n + 1)
            .unwrap_or(0);
        diff.truncate(cut);
        truncated = true;
    }
    if !truncated {
        let untracked = run_git_limited(
            &root,
            &["ls-files", "--others", "--exclude-standard", "-z"],
            MAX_DIFF_TOTAL_BYTES,
        ).await?;
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

        assert!(read_untracked_file(&root, "secret-link.txt").await.is_none());

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
}
