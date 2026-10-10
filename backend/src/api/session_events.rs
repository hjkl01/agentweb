use super::sessions::load_session;
use crate::state::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::{Duration, Utc};
use sqlx::Row;

/// Return recent persisted activity so completed sessions can restore their
/// execution timeline after a page refresh. Live sessions use WebSocket replay
/// instead, which avoids applying the same current-turn events twice.
pub async fn list_session_activity(
    Path(id): Path<String>,
    State(s): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, StatusCode> {
    load_session(&s.db, &id).await?;

    let cutoff = (Utc::now() - Duration::days(1)).to_rfc3339();
    let rows = sqlx::query(
        "SELECT payload FROM agent_events
         WHERE session_id=? AND created_at>=?
         ORDER BY id DESC LIMIT 500",
    )
    .bind(&id)
    .bind(cutoff)
    .fetch_all(&s.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let events = rows
        .into_iter()
        .rev()
        .filter_map(|row| {
            let payload = row.get::<String, _>(0);
            match serde_json::from_str::<serde_json::Value>(&payload) {
                Ok(event) => Some(event),
                Err(error) => {
                    tracing::warn!(session_id = %id, %error, "skipping malformed persisted agent event");
                    None
                }
            }
        })
        .collect();

    Ok(Json(events))
}
