use super::sessions::load_session;
use crate::agents::pi_transcript::{read_messages_from_workspace, TranscriptMessage};
use crate::state::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::{Duration, Utc};
use serde::Deserialize;

const SESSION_LEASE_SECONDS: i64 = 30;

// Bound concurrent filesystem scans and JSON parsing per backend process.
// Requests wait asynchronously instead of creating an unbounded queue of
// blocking transcript readers.
static TRANSCRIPT_READ_LIMIT: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);

pub async fn list_messages(
    Path(id): Path<String>,
    State(s): State<AppState>,
) -> Result<Json<Vec<TranscriptMessage>>, StatusCode> {
    // Conversation content belongs to the Agent's native session, not SQLite.
    let session = load_session(&s.db, &id).await?;
    // sessions.agent_id is a database row ID, not the adapter kind. Resolve
    // the kind before deciding whether this session uses Pi's JSONL transcript.
    let agent_kind: Option<String> = sqlx::query_scalar("SELECT kind FROM agents WHERE id=?")
        .bind(&session.agent_id)
        .fetch_optional(&s.db)
        .await
        .map_err(|error| {
            tracing::error!(session_id = %id, %error, "failed to resolve session agent kind");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    if agent_kind.as_deref() != Some("pi") {
        return Ok(Json(Vec::new()));
    }
    let Some(native_session_id) = session.native_session_id.as_deref() else {
        return Ok(Json(Vec::new()));
    };
    // Reading Pi JSONL is synchronous filesystem traversal plus JSON parsing.
    // Keep that work off Tokio's async executor so frequent transcript polling
    // cannot stall unrelated API requests.
    let permit = TRANSCRIPT_READ_LIMIT
        .acquire()
        .await
        .map_err(|error| {
            tracing::error!(%error, "Pi transcript read limiter is unavailable");
            StatusCode::SERVICE_UNAVAILABLE
        })?;
    let workspace = session.workspace.clone();
    let native_session_id = native_session_id.to_owned();
    let messages = tokio::task::spawn_blocking(move || {
        // Keep the permit for the entire blocking read, including if the HTTP
        // request is cancelled after the worker has started.
        let _permit = permit;
        read_messages_from_workspace(std::path::Path::new(&workspace), &native_session_id)
    })
    .await
    .map_err(|error| {
        tracing::error!(%error, "Pi transcript reader task failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(messages))
}

#[derive(Deserialize)]
pub struct SendMessage {
    pub message: String,
}


pub async fn send_message(
    Path(id): Path<String>,
    State(s): State<AppState>,
    Json(v): Json<SendMessage>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if v.message.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let now = Utc::now();
    let now_text = now.to_rfc3339();
    let lease_until = (now + Duration::seconds(SESSION_LEASE_SECONDS)).to_rfc3339();
    let worker_id = format!("{}:{}", s.worker_id, uuid::Uuid::new_v4());
    let claimed=sqlx::query("UPDATE sessions SET status='running',worker_id=?,lease_until=?,updated_at=? WHERE id=? AND status!='missing' AND (status!='running' OR lease_until IS NULL OR (julianday(lease_until) IS NOT NULL AND julianday(lease_until)<julianday(?)))")
        .bind(&worker_id).bind(&lease_until).bind(&now_text).bind(&id).bind(&now_text).execute(&s.db).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if claimed.rows_affected() == 0 {
        load_session(&s.db, &id).await?;
        return Err(StatusCode::CONFLICT);
    }
    let session = match load_session(&s.db, &id).await {
        Ok(session) => session,
        Err(status) => {
            // The claim above already marked this session as running. If loading
            // it fails, release that claim so the session is not stuck until the
            // lease expires. The ownership predicate protects a newer worker.
            if let Err(error) = sqlx::query("UPDATE sessions SET status='error',worker_id=NULL,lease_until=NULL,updated_at=? WHERE id=? AND status='running' AND worker_id=?")
                .bind(Utc::now().to_rfc3339())
                .bind(&id)
                .bind(&worker_id)
                .execute(&s.db)
                .await
            {
                tracing::error!(
                    session_id = %id,
                    worker_id = %worker_id,
                    %error,
                    "failed to release session claim after loading the session failed"
                );
            }
            return Err(status);
        }
    };
    tracing::info!(session_id = %id, agent_id = %session.agent_id, worker_id = %worker_id, message_bytes = v.message.len(), "accepted chat message");
    let title = if session.title.trim().is_empty() || session.title == "New Chat" {
        let title = format!("Chat {}", now.format("%Y-%m-%d %H:%M"));
        if sqlx::query("UPDATE sessions SET title=?,updated_at=? WHERE id=? AND title='New Chat'")
            .bind(&title)
            .bind(&now_text)
            .bind(&id)
            .execute(&s.db)
            .await
            .is_err()
        {
            let _ = sqlx::query("UPDATE sessions SET status='error',worker_id=NULL,lease_until=NULL,updated_at=? WHERE id=? AND status='running' AND worker_id=?")
                .bind(Utc::now().to_rfc3339())
                .bind(&id)
                .bind(&worker_id)
                .execute(&s.db)
                .await;
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
        title
    } else {
        session.title.clone()
    };

    let events = s.events.clone();
    let db = s.db.clone();
    let agents = s.agents.clone();
    let execution_session_id = session.id.clone();
    tokio::spawn(async move {
        tracing::info!(session_id = %execution_session_id, worker_id = %worker_id, "starting agent session execution");
        crate::agents::process::run_session(db, agents, session, v.message, events, worker_id)
            .await;
        tracing::info!(session_id = %execution_session_id, "agent session execution task returned");
    });
    Ok(Json(serde_json::json!({"status":"started", "title":title})))
}

pub async fn interrupt(
    Path(id): Path<String>,
    State(s): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let session = load_session(&s.db, &id).await?;
    let agent_kind: String = sqlx::query_scalar("SELECT kind FROM agents WHERE id=?")
        .bind(&session.agent_id)
        .fetch_optional(&s.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .unwrap_or(session.agent_id);
    // Capture the current execution token first. The conditional update below
    // prevents a concurrent lease takeover from being interrupted by this request.
    let execution_id: Option<String> = sqlx::query_scalar(
        "SELECT worker_id FROM sessions WHERE id=? AND status='running'",
    )
    .bind(&id)
    .fetch_optional(&s.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .flatten();
    let Some(execution_id) = execution_id else {
        return Ok(Json(
            serde_json::json!({"status":load_session(&s.db,&id).await?.status}),
        ));
    };

    // Stop the exact process first. Do not clear the database lease before
    // interruption succeeds, otherwise a failed stop can leave a live process
    // whose owner can no longer renew or finalize the session.
    s.agents
        .adapter(&agent_kind)
        .await
        .interrupt(&id, &execution_id)
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let now = Utc::now().to_rfc3339();
    let changed = sqlx::query(
        "UPDATE sessions SET status='interrupted',worker_id=NULL,lease_until=NULL,updated_at=? WHERE id=? AND status='running' AND worker_id=?",
    )
    .bind(&now)
    .bind(&id)
    .bind(&execution_id)
    .execute(&s.db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if changed.rows_affected() == 0 {
        return Ok(Json(
            serde_json::json!({"status":load_session(&s.db,&id).await?.status}),
        ));
    }
    Ok(Json(serde_json::json!({"status":"interrupted"})))
}
