use crate::state::AppState;
use super::sessions::load_session;
use axum::{extract::{Path, State}, http::StatusCode, Json};
use chrono::{Duration, Utc};
use serde::Deserialize;
use sqlx::Row;
use uuid::Uuid;

const SESSION_LEASE_SECONDS: i64 = 30;

pub async fn list_messages(Path(id): Path<String>, State(s): State<AppState>) -> Result<Json<Vec<serde_json::Value>>, StatusCode> {
    load_session(&s.db, &id).await?;
    let rows = sqlx::query("SELECT id,role,content,created_at FROM messages WHERE session_id=? ORDER BY created_at")
        .bind(&id).fetch_all(&s.db).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(rows.into_iter().map(|r| serde_json::json!({"id":r.get::<String,_>(0),"role":r.get::<String,_>(1),"content":r.get::<String,_>(2),"created_at":r.get::<String,_>(3)})).collect()))
}

#[derive(Deserialize)]
pub struct SendMessage { pub message: String }

fn build_title(message: &str) -> String {
    let normalized = message.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut title = normalized.chars().take(40).collect::<String>();
    if normalized.chars().count() > 40 { title.push('…'); }
    if title.is_empty() { "New Chat".into() } else { title }
}

pub async fn send_message(Path(id): Path<String>, State(s): State<AppState>, Json(v): Json<SendMessage>) -> Result<Json<serde_json::Value>, StatusCode> {
    if v.message.trim().is_empty() { return Err(StatusCode::BAD_REQUEST); }
    let now=Utc::now();
    let now_text=now.to_rfc3339();
    let lease_until=(now+Duration::seconds(SESSION_LEASE_SECONDS)).to_rfc3339();
    let claimed=sqlx::query("UPDATE sessions SET status='running',worker_id=?,lease_until=?,updated_at=? WHERE id=? AND (status!='running' OR lease_until IS NULL OR lease_until<?)")
        .bind(&s.worker_id).bind(&lease_until).bind(&now_text).bind(&id).bind(&now_text).execute(&s.db).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if claimed.rows_affected()==0 { load_session(&s.db,&id).await?; return Err(StatusCode::CONFLICT); }
    let session=load_session(&s.db,&id).await?;
    if sqlx::query("INSERT INTO messages(id,session_id,role,content,created_at) VALUES(?,?,?,?,?)").bind(Uuid::new_v4().to_string()).bind(&id).bind("user").bind(&v.message).bind(&now_text).execute(&s.db).await.is_err() {
        let _=sqlx::query("UPDATE sessions SET status='error',lease_until=NULL,updated_at=? WHERE id=? AND status='running' AND worker_id=?").bind(Utc::now().to_rfc3339()).bind(&id).bind(&s.worker_id).execute(&s.db).await;
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    let title = if session.title.trim().is_empty() || session.title == "New Chat" {
        let title = build_title(&v.message);
        sqlx::query("UPDATE sessions SET title=?,updated_at=? WHERE id=? AND title='New Chat'")
            .bind(&title).bind(&now_text).bind(&id).execute(&s.db).await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        title
    } else { session.title.clone() };

    let events=s.events.clone(); let db=s.db.clone(); let agents=s.agents.clone(); let worker_id=s.worker_id.clone();
    tokio::spawn(async move { crate::agents::process::run_session(db,agents,session,v.message,events,worker_id).await; });
    Ok(Json(serde_json::json!({"status":"started", "title":title})))
}

pub async fn interrupt(Path(id): Path<String>, State(s): State<AppState>) -> Result<Json<serde_json::Value>, StatusCode> {
    let now=Utc::now().to_rfc3339();
    let changed=sqlx::query("UPDATE sessions SET status='interrupted',updated_at=? WHERE id=? AND status='running'").bind(&now).bind(&id).execute(&s.db).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if changed.rows_affected()==0 { return Ok(Json(serde_json::json!({"status":load_session(&s.db,&id).await?.status}))); }
    s.agents.interrupt(&id).await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok(Json(serde_json::json!({"status":"interrupted"})))
}
