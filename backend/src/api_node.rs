use crate::{installation::runtime, state::AppState};
use axum::{extract::State, Json};
use serde::Serialize;
use sqlx::Row;

#[derive(Serialize)]
pub struct NodeVersions {
    pub available: Vec<&'static str>,
    pub installed: Vec<String>,
    pub detected: Vec<runtime::DetectedNode>,
    pub active: Option<String>,
    pub configured_path: Option<String>,
}

pub async fn node_versions(State(s): State<AppState>) -> Json<NodeVersions> {
    let installed = runtime::installed_versions().await.unwrap_or_default();
    let detected = runtime::detect_nodes().await.unwrap_or_default();
    let configured_path = sqlx::query("SELECT value FROM runtime_settings WHERE key=?")
        .bind("node_path")
        .fetch_optional(&s.db)
        .await
        .ok()
        .flatten()
        .map(|row| row.get::<String, _>(0))
        .filter(|p| !p.trim().is_empty());

    let active = configured_path
        .as_ref()
        .and_then(|path| {
            detected
                .iter()
                .find(|node| node.path == *path)
                .map(|node| node.version.clone())
        })
        .or_else(|| installed.first().cloned())
        .or_else(|| detected.first().map(|node| node.version.clone()));

    Json(NodeVersions {
        available: runtime::supported_node_versions(),
        installed,
        detected,
        active,
        configured_path,
    })
}
