use super::sessions::get_session;
use crate::state::AppState;
use axum::{
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::path::{Component, PathBuf};
use tokio::fs;

#[derive(Deserialize)]
pub struct TreeQuery {
    pub path: Option<String>,
    pub search: Option<String>,
}
#[derive(Serialize)]
pub struct TreeItem {
    pub name: String,
    pub path: String,
    pub kind: &'static str,
    pub size: u64,
}

fn validate_relative(path: &str) -> Result<PathBuf, StatusCode> {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        return Err(StatusCode::BAD_REQUEST);
    }
    for component in path.components() {
        if matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        ) {
            return Err(StatusCode::BAD_REQUEST);
        }
    }
    Ok(path)
}

fn ignored(name: &str) -> bool {
    matches!(name, ".git" | "node_modules" | "target")
}

async fn resolve_tree_path(root: &PathBuf, relative: &str) -> Result<PathBuf, StatusCode> {
    let relative = validate_relative(relative)?;
    let mut candidate = root.clone();

    // Reject symlinks in every component, including links that point back
    // inside the workspace.
    for component in relative.components() {
        if let Component::Normal(name) = component {
            candidate.push(name);
            let metadata = fs::symlink_metadata(&candidate)
                .await
                .map_err(|_| StatusCode::NOT_FOUND)?;
            if metadata.file_type().is_symlink() {
                return Err(StatusCode::FORBIDDEN);
            }
        }
    }

    let canonical = fs::canonicalize(&candidate)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    if !canonical.starts_with(root) {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(canonical)
}

async fn search_tree(root: &PathBuf, needle: &str) -> Result<Vec<TreeItem>, StatusCode> {
    let needle = needle.to_lowercase();
    let mut pending = vec![root.clone()];
    let mut result = Vec::new();
    let mut scanned_entries = 0usize;
    const MAX_SCANNED_ENTRIES: usize = 20_000;
    while let Some(dir) = pending.pop() {
        if scanned_entries >= MAX_SCANNED_ENTRIES {
            break;
        }
        let mut entries = fs::read_dir(&dir)
            .await
            .map_err(|_| StatusCode::NOT_FOUND)?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        {
            scanned_entries += 1;
            if result.len() >= 2000 || scanned_entries >= MAX_SCANNED_ENTRIES {
                break;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if ignored(&name) {
                continue;
            }
            // Do not traverse symlinks, even when they point back inside the
            // workspace: directory links can create cycles and unbounded scans.
            if entry.file_type().await.map(|kind| kind.is_symlink()).unwrap_or(true) {
                continue;
            }
            let canonical = match fs::canonicalize(entry.path()).await {
                Ok(p) => p,
                Err(_) => continue,
            };
            if !canonical.starts_with(root) {
                continue;
            }
            let metadata = match fs::metadata(&canonical).await {
                Ok(m) => m,
                Err(_) => continue,
            };
            let relative = match canonical.strip_prefix(root) {
                Ok(p) => p.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };
            if relative.to_lowercase().contains(&needle) {
                result.push(TreeItem {
                    name: name.clone(),
                    path: relative,
                    kind: if metadata.is_dir() {
                        "directory"
                    } else {
                        "file"
                    },
                    size: metadata.len(),
                });
            }
            if metadata.is_dir() {
                pending.push(canonical);
            }
        }
        if result.len() >= 2000 {
            break;
        }
    }
    result.sort_by(|a, b| a.path.to_lowercase().cmp(&b.path.to_lowercase()));
    Ok(result)
}

pub async fn tree(
    AxumPath(id): AxumPath<String>,
    State(state): State<AppState>,
    Query(query): Query<TreeQuery>,
) -> Result<Json<Vec<TreeItem>>, StatusCode> {
    let session = get_session(AxumPath(id), State(state))
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?
        .0;
    let root = fs::canonicalize(&session.workspace)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    if let Some(search) = query
        .search
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Ok(Json(search_tree(&root, search).await?));
    }
    let current = match query.path.as_deref() {
        None | Some("") => root.clone(),
        Some(relative) => resolve_tree_path(&root, relative).await?,
    };
    let metadata = fs::metadata(&current)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    if !metadata.is_dir() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut entries = fs::read_dir(&current)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let mut result = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        let name = entry.file_name().to_string_lossy().into_owned();
        if ignored(&name) {
            continue;
        }
        if entry.file_type().await.map(|kind| kind.is_symlink()).unwrap_or(true) {
            continue;
        }
        let metadata = match entry.metadata().await {
            Ok(m) => m,
            Err(_) => continue,
        };
        let relative = entry
            .path()
            .strip_prefix(&root)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .to_string_lossy()
            .replace('\\', "/");
        result.push(TreeItem {
            name,
            path: relative,
            kind: if metadata.is_dir() {
                "directory"
            } else {
                "file"
            },
            size: metadata.len(),
        });
        if result.len() >= 2000 {
            break;
        }
    }
    result.sort_by(|a, b| {
        a.kind
            .cmp(b.kind)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(Json(result))
}

#[cfg(test)]
mod symlink_tree_tests {
    use super::*;
    use uuid::Uuid;

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("agentweb-{label}-{}", Uuid::new_v4()))
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn tree_rejects_symlink_path_components() {
        use std::os::unix::fs::symlink;

        let root = temp_root("tree-path-root");
        let outside = temp_root("tree-path-outside");
        std::fs::create_dir_all(root.join("nested")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), "not visible").unwrap();
        symlink(&outside, root.join("linked-dir")).unwrap();
        symlink(root.join("nested"), root.join("linked-internal-dir")).unwrap();

        let root = fs::canonicalize(root).await.unwrap();
        assert_eq!(
            resolve_tree_path(&root, "linked-dir").await,
            Err(StatusCode::FORBIDDEN)
        );
        assert_eq!(
            resolve_tree_path(&root, "linked-internal-dir").await,
            Err(StatusCode::FORBIDDEN)
        );

        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn search_tree_skips_symlinks_and_directory_cycles() {
        use std::os::unix::fs::symlink;

        let root = temp_root("tree-root");
        let outside = temp_root("tree-outside");
        std::fs::create_dir_all(root.join("nested")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("outside-secret.txt"), "not visible").unwrap();
        symlink(&outside, root.join("outside-link")).unwrap();
        symlink(&root, root.join("nested/back-to-root")).unwrap();

        let result = search_tree(&root, "outside-secret").await.unwrap();
        assert!(result.is_empty(), "search must not follow an external symlink");
        let result = search_tree(&root, "nested").await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].path, "nested");

        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }
}
