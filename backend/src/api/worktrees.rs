//! Git worktree discovery and creation for a workspace directory.
//!
//! Agent Web has no "switch worktree" action on a live session: a worktree is
//! used by creating a new chat whose workspace points at it. These endpoints
//! expose that path plus the project/branch context the picker needs.
//!
//! Every operation is gated by the configured workspace root
//! (`AGENTWEB_WORKSPACE_DIR`, default `$HOME`), the same boundary as session
//! creation. Removal is intentionally not exposed here yet.
use super::sessions::canonical_home;
use crate::state::AppState;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path as FsPath, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::process::Command;

const SHORT_TIMEOUT: Duration = Duration::from_secs(10);
// Large repositories can take minutes to check out.
const CHECKOUT_TIMEOUT: Duration = Duration::from_secs(5 * 60);

#[derive(Serialize)]
pub struct WorktreeInfo {
    pub path: String,
    pub branch: Option<String>,
    pub is_main: bool,
    pub available: bool,
}

#[derive(Serialize)]
pub struct WorktreesView {
    pub is_git: bool,
    pub project_root: Option<String>,
    pub worktrees: Vec<WorktreeInfo>,
}

/// Run git with a hard timeout. `kill_on_drop` ensures a timed-out checkout
/// does not leave a git process behind after the request fails.
async fn git(
    root: &FsPath,
    args: &[&str],
    timeout: Duration,
) -> Result<(bool, String, String), StatusCode> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(root)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .kill_on_drop(true);
    match tokio::time::timeout(timeout, command.output()).await {
        Ok(Ok(output)) => Ok((
            output.status.success(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )),
        Ok(Err(error)) => {
            tracing::error!(workspace = %root.display(), %error, "failed to run git");
            Err(StatusCode::BAD_GATEWAY)
        }
        Err(_) => Err(StatusCode::GATEWAY_TIMEOUT),
    }
}

/// Canonicalize a cwd and require it to live under the workspace root.
pub(super) async fn allowed_cwd(cwd: &str) -> Result<PathBuf, StatusCode> {
    let cwd = cwd.trim();
    if cwd.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let home = canonical_home().await?;
    let path = tokio::fs::canonicalize(cwd)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    if !path.is_dir() || !path.starts_with(&home) {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(path)
}

/// The repository a worktree's cwd belongs to: the parent of the shared
/// `.git` dir, or the dir itself for a bare clone.
fn repo_root_from_common_dir(common: &FsPath) -> PathBuf {
    if common.file_name().and_then(|name| name.to_str()) == Some(".git") {
        common
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| common.to_path_buf())
    } else {
        common.to_path_buf()
    }
}

async fn repo_root(cwd: &FsPath) -> Option<PathBuf> {
    let (ok, stdout, _) = git(
        cwd,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        SHORT_TIMEOUT,
    )
    .await
    .ok()?;
    if !ok {
        return None;
    }
    let printed = stdout.trim();
    if printed.is_empty() {
        return None;
    }
    let root = repo_root_from_common_dir(FsPath::new(printed));
    Some(tokio::fs::canonicalize(&root).await.unwrap_or(root))
}

fn parse_worktrees(porcelain: &str) -> Vec<WorktreeInfo> {
    let mut worktrees = Vec::new();
    let mut current: Option<(String, Option<String>, bool)> = None;
    let flush = |entry: Option<(String, Option<String>, bool)>,
                 worktrees: &mut Vec<WorktreeInfo>| {
        if let Some((path, branch, prunable)) = entry {
            if !prunable {
                worktrees.push(WorktreeInfo {
                    available: std::fs::metadata(&path)
                        .map(|m| m.is_dir())
                        .unwrap_or(false),
                    is_main: worktrees.is_empty(),
                    path,
                    branch,
                });
            }
        }
    };
    for line in porcelain.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            flush(current.take(), &mut worktrees);
            current = Some((path.trim().to_owned(), None, false));
        } else if let (Some(branch), Some((_, current_branch, _))) =
            (line.strip_prefix("branch "), current.as_mut())
        {
            *current_branch = Some(branch.trim().replacen("refs/heads/", "", 1));
        } else if line.starts_with("prunable") {
            if let Some((_, _, prunable)) = current.as_mut() {
                *prunable = true;
            }
        }
    }
    flush(current, &mut worktrees);
    worktrees
}

fn sanitize_branch_for_dir(branch: &str) -> String {
    branch
        .replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "-")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
        .trim_matches('-')
        .to_owned()
}

pub async fn list_worktrees(
    Query(query): Query<CwdQuery>,
) -> Result<Json<WorktreesView>, StatusCode> {
    let cwd = allowed_cwd(query.cwd.as_deref().unwrap_or_default()).await?;
    let Some(root) = repo_root(&cwd).await else {
        return Ok(Json(WorktreesView {
            is_git: false,
            project_root: None,
            worktrees: Vec::new(),
        }));
    };
    let (ok, stdout, _) = git(&cwd, &["worktree", "list", "--porcelain"], SHORT_TIMEOUT).await?;
    let worktrees = if ok {
        parse_worktrees(&stdout)
    } else {
        Vec::new()
    };
    Ok(Json(WorktreesView {
        is_git: true,
        project_root: Some(root.to_string_lossy().into_owned()),
        worktrees,
    }))
}

#[derive(Deserialize)]
pub struct CwdQuery {
    pub cwd: Option<String>,
}

#[derive(Deserialize)]
pub struct AddWorktree {
    pub cwd: String,
    pub branch: String,
}

pub async fn add_worktree(
    State(_state): State<AppState>,
    Json(input): Json<AddWorktree>,
) -> Result<Json<WorktreeInfo>, (StatusCode, Json<serde_json::Value>)> {
    let cwd = allowed_cwd(&input.cwd)
        .await
        .map_err(|status| error(status, "工作目录不在允许范围内"))?;
    let branch = input.branch.trim();
    if branch.is_empty() || branch.len() > 200 || branch.starts_with('-') || branch.contains("..") {
        return Err(error(StatusCode::BAD_REQUEST, "分支名无效"));
    }
    let home = canonical_home()
        .await
        .map_err(|status| error(status, "无法解析工作目录根"))?;
    let root = repo_root(&cwd)
        .await
        .ok_or_else(|| error(StatusCode::BAD_REQUEST, "该目录不是 Git 仓库"))?;
    if !root.starts_with(&home) {
        return Err(error(StatusCode::FORBIDDEN, "仓库不在允许范围内"));
    }
    let base = PathBuf::from(format!("{}-worktrees", root.display()));
    if !base.starts_with(&home) {
        return Err(error(StatusCode::FORBIDDEN, "worktree 目录不在允许范围内"));
    }
    let dir_name = sanitize_branch_for_dir(branch);
    if dir_name.is_empty() {
        return Err(error(StatusCode::BAD_REQUEST, "分支名无法作为目录名"));
    }
    let path = base.join(&dir_name);
    if tokio::fs::symlink_metadata(&path).await.is_ok() {
        return Err(error(StatusCode::CONFLICT, "目标目录已存在"));
    }
    tokio::fs::create_dir_all(&base)
        .await
        .map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "无法创建工作目录"))?;

    let path_str = path.to_string_lossy().into_owned();
    let branch_ref = format!("refs/heads/{branch}");
    let (exists, _, _) = git(
        &root,
        &["rev-parse", "--verify", "--quiet", &branch_ref],
        SHORT_TIMEOUT,
    )
    .await
    .map_err(|status| error(status, "无法检查分支是否存在"))?;
    // Reuse an existing branch, otherwise create it from the current HEAD.
    // No fetch: worktree creation stays a local, offline-safe operation.
    let args: Vec<&str> = if exists {
        vec!["worktree", "add", "--", &path_str, branch]
    } else {
        vec!["worktree", "add", "-b", branch, "--", &path_str]
    };
    let (ok, _, stderr) = git(&root, &args, CHECKOUT_TIMEOUT)
        .await
        .map_err(|status| error(status, "git worktree 执行失败"))?;
    if !ok {
        tracing::warn!(workspace = %root.display(), %stderr, "git worktree add failed");
        let message = if stderr.contains("already exists") {
            "分支或目录已存在"
        } else {
            stderr.trim()
        };
        return Err(error(StatusCode::BAD_REQUEST, message));
    }
    let canonical = tokio::fs::canonicalize(&path)
        .await
        .map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "worktree 创建后不可访问"))?;
    if !canonical.starts_with(&home) {
        let _ = git(
            &root,
            &["worktree", "remove", "--force", &path_str],
            SHORT_TIMEOUT,
        )
        .await;
        return Err(error(StatusCode::FORBIDDEN, "worktree 路径越界"));
    }
    Ok(Json(WorktreeInfo {
        path: canonical.to_string_lossy().into_owned(),
        branch: Some(branch.to_owned()),
        is_main: false,
        available: true,
    }))
}

fn error(status: StatusCode, message: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    error_code(status, "WORKTREE", message)
}

fn error_code(
    status: StatusCode,
    code: &str,
    message: impl Into<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({"error": {"code": code, "message": message.into()}})),
    )
}

/// Git refuses to remove a dirty worktree, or one with initialized submodules,
/// without `--force`. Surface that so the UI can offer a forced retry.
fn removal_requires_force(stderr: &str) -> bool {
    let lower = stderr.to_lowercase();
    lower.contains("contains modified or untracked files")
        || lower.contains("is dirty")
        || lower.contains("containing submodules")
}

#[derive(Deserialize)]
pub struct RemoveWorktree {
    pub cwd: String,
    pub path: String,
    #[serde(default)]
    pub force: bool,
}

#[derive(Serialize)]
pub struct RemoveResult {
    pub success: bool,
}

pub async fn remove_worktree(
    Json(input): Json<RemoveWorktree>,
) -> Result<Json<RemoveResult>, (StatusCode, Json<serde_json::Value>)> {
    let cwd = allowed_cwd(&input.cwd)
        .await
        .map_err(|status| error(status, "工作目录不在允许范围内"))?;
    if input.path.trim().is_empty() {
        return Err(error(StatusCode::BAD_REQUEST, "缺少 worktree 路径"));
    }
    let home = canonical_home()
        .await
        .map_err(|status| error(status, "无法解析工作目录根"))?;
    let target = tokio::fs::canonicalize(&input.path)
        .await
        .map_err(|_| error(StatusCode::NOT_FOUND, "worktree 不存在"))?;
    if !target.starts_with(&home) {
        return Err(error(StatusCode::FORBIDDEN, "worktree 路径越界"));
    }
    let (ok, stdout, _) = git(&cwd, &["worktree", "list", "--porcelain"], SHORT_TIMEOUT)
        .await
        .map_err(|status| error(status, "无法列出 worktree"))?;
    if !ok {
        return Err(error(StatusCode::BAD_REQUEST, "该目录不是 Git 仓库"));
    }
    let worktrees = parse_worktrees(&stdout);
    let found = worktrees.iter().find(|item| {
        std::fs::canonicalize(&item.path)
            .map(|path| path == target)
            .unwrap_or(false)
    });
    match found {
        None => return Err(error(StatusCode::NOT_FOUND, "不是本仓库的 worktree")),
        Some(item) if item.is_main => {
            return Err(error(StatusCode::BAD_REQUEST, "不能删除主 worktree"));
        }
        Some(_) => {}
    }

    let target_str = target.to_string_lossy().into_owned();
    let args: Vec<&str> = if input.force {
        vec!["worktree", "remove", "--force", &target_str]
    } else {
        vec!["worktree", "remove", &target_str]
    };
    let (ok, _, stderr) = git(&cwd, &args, CHECKOUT_TIMEOUT)
        .await
        .map_err(|status| error(status, "git worktree remove 执行失败"))?;
    if !ok {
        tracing::warn!(%stderr, "git worktree remove failed");
        if removal_requires_force(&stderr) {
            return Err(error_code(
                StatusCode::CONFLICT,
                "WORKTREE_DIRTY",
                "worktree 有未提交的改动或已初始化的子模块",
            ));
        }
        return Err(error(StatusCode::BAD_REQUEST, stderr.trim()));
    }
    Ok(Json(RemoveResult { success: true }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_branch_names_for_directories() {
        assert_eq!(sanitize_branch_for_dir("feature/foo"), "feature-foo");
        assert_eq!(sanitize_branch_for_dir("a b  c"), "a-b-c");
        assert_eq!(sanitize_branch_for_dir("--x--"), "x");
    }

    #[test]
    fn derives_repo_root_from_common_dir() {
        assert_eq!(
            repo_root_from_common_dir(FsPath::new("/home/u/repo/.git")),
            PathBuf::from("/home/u/repo")
        );
        assert_eq!(
            repo_root_from_common_dir(FsPath::new("/srv/repo.git")),
            PathBuf::from("/srv/repo.git")
        );
    }

    #[test]
    fn detects_when_removal_needs_force() {
        assert!(removal_requires_force(
            "fatal: '/tmp/wt' contains modified or untracked files, use --force to delete it"
        ));
        assert!(removal_requires_force(
            "error: failed to delete '/tmp/wt': working tree is dirty"
        ));
        assert!(removal_requires_force(
            "fatal: '/tmp/wt' is a worktree containing submodules, cannot remove without --force"
        ));
        assert!(!removal_requires_force("fatal: not a git repository"));
    }

    #[test]
    fn parses_worktree_porcelain_and_skips_prunable() {
        let porcelain = "\
worktree /home/u/repo
HEAD abc
branch refs/heads/main

worktree /home/u/repo-worktrees/feature
HEAD def
branch refs/heads/feature

worktree /home/u/repo-worktrees/gone
HEAD 123
branch refs/heads/gone
prunable gitdir file points to non-existent location
";
        let worktrees = parse_worktrees(porcelain);
        assert_eq!(worktrees.len(), 2);
        assert_eq!(worktrees[0].path, "/home/u/repo");
        assert!(worktrees[0].is_main);
        assert_eq!(worktrees[1].branch.as_deref(), Some("feature"));
        assert!(!worktrees[1].is_main);
    }
}
