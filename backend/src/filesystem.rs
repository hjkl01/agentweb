use axum::{extract::Query, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;

#[derive(Deserialize)]
pub struct TreeQuery {
    pub path: Option<String>,
}

#[derive(Serialize)]
pub struct TreeItem {
    pub name: String,
    pub path: String,
    pub kind: &'static str,
    pub size: u64,
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| PathBuf::from("/root"))
}

fn inside_home(home: &Path, path: &Path) -> bool {
    path.starts_with(home)
}

pub async fn tree(Query(query): Query<TreeQuery>) -> Result<Json<Vec<TreeItem>>, StatusCode> {
    let home = fs::canonicalize(home_dir())
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let current = match query.path.as_deref() {
        None | Some("") => home.clone(),
        Some(path) => {
            let requested = PathBuf::from(path);
            let absolute = if requested.is_absolute() {
                requested
            } else {
                home.join(requested)
            };
            let canonical = fs::canonicalize(absolute)
                .await
                .map_err(|_| StatusCode::NOT_FOUND)?;
            if !inside_home(&home, &canonical) {
                return Err(StatusCode::FORBIDDEN);
            }
            canonical
        }
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
        let path = entry.path();
        let metadata = match entry.metadata().await {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == ".git" || name == "node_modules" || name == "target" {
            continue;
        }

        result.push(TreeItem {
            name,
            path: path.to_string_lossy().into_owned(),
            kind: if metadata.is_dir() { "directory" } else { "file" },
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
