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
    // Restore only the latest turn. Include its start event plus the newest
    // 499 following events so token deltas from older turns cannot overwhelm the
    // activity list; a very long turn may omit intermediate events, but keeps its
    // beginning and latest output.
    let rows = sqlx::query(
        "WITH latest_start AS (
             SELECT MAX(id) AS id FROM agent_events
             WHERE session_id=? AND event_type='message.started' AND created_at>=?
         ),
         recent AS (
             SELECT id,payload FROM agent_events
             WHERE session_id=? AND created_at>=?
               AND id>COALESCE((SELECT id FROM latest_start),0)
             ORDER BY id DESC LIMIT 499
         ),
         start_event AS (
             SELECT e.id,e.payload FROM agent_events e
             JOIN latest_start s ON e.id=s.id
         )
         SELECT payload FROM (
             SELECT id,payload FROM recent
             UNION ALL
             SELECT id,payload FROM start_event
         ) ORDER BY id",
    )
    .bind(&id)
    .bind(&cutoff)
    .bind(&id)
    .bind(&cutoff)
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
