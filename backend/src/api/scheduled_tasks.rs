use crate::state::AppState;
use axum::{extract::{Path, Query, State}, http::StatusCode, Json};
use chrono::{Datelike, Timelike, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct TaskInput {
    pub name: String,
    pub prompt: String,
    pub agent_id: String,
    pub working_directory: String,
    pub model: Option<String>,
    pub schedule_type: String,
    pub schedule_config: Value,
    #[serde(default = "default_timezone")]
    pub timezone: String,
    #[serde(default = "default_strategy")]
    pub session_strategy: String,
    pub reuse_session_id: Option<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}
fn default_timezone() -> String { "UTC".into() }
fn default_strategy() -> String { "new".into() }
fn default_enabled() -> bool { true }

fn validate(v: &TaskInput) -> Result<(), StatusCode> {
    if v.name.trim().is_empty() || v.name.chars().count() > 120 || v.prompt.trim().is_empty()
        || v.agent_id.trim().is_empty() || v.working_directory.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if !matches!(v.schedule_type.as_str(), "once" | "interval" | "cron") { return Err(StatusCode::BAD_REQUEST); }
    match v.schedule_type.as_str() {
        "once" => {
            if v.schedule_config.get("at").and_then(Value::as_str).and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).is_none() { return Err(StatusCode::BAD_REQUEST); }
        }
        "interval" => {
            if !v.schedule_config.get("seconds").and_then(Value::as_i64).is_some_and(|s| s >= 1) { return Err(StatusCode::BAD_REQUEST); }
        }
        "cron" => {
            let expr = v.schedule_config.get("expression").and_then(Value::as_str).unwrap_or("");
            if expr.split_whitespace().count() != 5 || !matches!(v.timezone.as_str(), "UTC" | "Etc/UTC") { return Err(StatusCode::BAD_REQUEST); }
        }
        _ => return Err(StatusCode::BAD_REQUEST),
    }
    if !matches!(v.session_strategy.as_str(), "new" | "reuse") { return Err(StatusCode::BAD_REQUEST); }
    match v.session_strategy.as_str() {
        "new" if v.reuse_session_id.is_some() => return Err(StatusCode::BAD_REQUEST),
        "reuse" if !v.reuse_session_id.as_deref().is_some_and(|s| !s.trim().is_empty()) => return Err(StatusCode::BAD_REQUEST),
        _ => {}
    }
    Ok(())
}

fn row_json(row: &sqlx::sqlite::SqliteRow) -> Value {
    let config: String = row.get("schedule_config");
    json!({
        "id": row.get::<String,_>("id"),
        "name": row.get::<String,_>("name"),
        "prompt": row.get::<String,_>("prompt"),
        "agent_id": row.get::<String,_>("agent_id"),
        "working_directory": row.get::<String,_>("working_directory"),
        "model": row.get::<Option<String>,_>("model"),
        "schedule_type": row.get::<String,_>("schedule_type"),
        "schedule_config": serde_json::from_str::<Value>(&config).unwrap_or(Value::Null),
        "timezone": row.get::<String,_>("timezone"),
        "session_strategy": row.get::<String,_>("session_strategy"),
        "reuse_session_id": row.get::<Option<String>,_>("reuse_session_id"),
        "enabled": row.get::<i64,_>("enabled") != 0,
        "next_run_at": row.get::<Option<String>,_>("next_run_at"),
        "last_run_at": row.get::<Option<String>,_>("last_run_at"),
        "created_at": row.get::<String,_>("created_at"),
        "updated_at": row.get::<String,_>("updated_at")
    })
}
const SELECT_TASK: &str = "SELECT id,name,prompt,agent_id,working_directory,model,schedule_type,schedule_config,timezone,session_strategy,reuse_session_id,enabled,next_run_at,last_run_at,created_at,updated_at FROM scheduled_tasks";

fn initial_next(v: &TaskInput) -> Result<Option<String>, StatusCode> {
    let now = Utc::now();
    let dt = match v.schedule_type.as_str() {
        "once" => chrono::DateTime::parse_from_rfc3339(v.schedule_config.get("at").and_then(Value::as_str).unwrap_or(""))
            .map_err(|_| StatusCode::BAD_REQUEST)?.with_timezone(&Utc),
        "interval" => now + chrono::Duration::seconds(v.schedule_config.get("seconds").and_then(Value::as_i64).ok_or(StatusCode::BAD_REQUEST)?),
        "cron" => {
            let fields: Vec<_> = v.schedule_config.get("expression").and_then(Value::as_str).unwrap_or("").split_whitespace().collect();
            let mut t = now.with_second(0).and_then(|t| t.with_nanosecond(0)).ok_or(StatusCode::BAD_REQUEST)? + chrono::Duration::minutes(1);
            let mut found = None;
            for _ in 0..60*24*366 {
                let vals = [t.minute(),t.hour(),t.day(),t.month(),t.weekday().num_days_from_sunday()];
                let ranges = [(0,59),(0,23),(1,31),(1,12),(0,6)];
                let ok = fields.iter().zip(vals.iter()).zip(ranges.iter()).all(|((f,val),(lo,hi))| {
                    f.split(',').any(|part| {
                        let (base,step) = part.split_once('/').map(|(a,b)|(a,b.parse::<u32>().unwrap_or(0))).unwrap_or((part,1));
                        if step == 0 { return false; }
                        let (a,b) = if base=="*" { (*lo,*hi) } else if let Some((a,b))=base.split_once('-') { (a.parse().unwrap_or(u32::MAX),b.parse().unwrap_or(0)) } else { let x=base.parse().unwrap_or(u32::MAX); (x,x) };
                        *val>=a && *val<=b && (*val-a)%step==0
                    })
                });
                if ok { found=Some(t); break; }
                t += chrono::Duration::minutes(1);
            }
            found.ok_or(StatusCode::BAD_REQUEST)?
        }
        _ => return Err(StatusCode::BAD_REQUEST),
    };
    if dt <= now { return Err(StatusCode::BAD_REQUEST); }
    Ok(Some(dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)))
}

pub async fn list(State(s): State<AppState>) -> Result<Json<Vec<Value>>, StatusCode> {
    let rows = sqlx::query(&format!("{SELECT_TASK} ORDER BY created_at DESC")).fetch_all(&s.db).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(rows.iter().map(row_json).collect()))
}
pub async fn get(Path(id): Path<String>, State(s): State<AppState>) -> Result<Json<Value>, StatusCode> {
    let row = sqlx::query(&format!("{SELECT_TASK} WHERE id=?")).bind(id).fetch_optional(&s.db).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?.ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(row_json(&row)))
}
pub async fn create(State(s): State<AppState>, Json(v): Json<TaskInput>) -> Result<Json<Value>, StatusCode> {
    validate(&v)?;
    crate::api::build_agent_config(&s.db, v.agent_id.trim()).await.map_err(|_| StatusCode::BAD_REQUEST)?;
    if v.session_strategy == "reuse" {
        let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE id=?").bind(v.reuse_session_id.as_deref().unwrap_or("")).fetch_one(&s.db).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if exists == 0 { return Err(StatusCode::BAD_REQUEST); }
    }
    let next = initial_next(&v)?;
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    sqlx::query("INSERT INTO scheduled_tasks(id,name,prompt,agent_id,working_directory,model,schedule_type,schedule_config,timezone,session_strategy,reuse_session_id,enabled,next_run_at,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)")
        .bind(&id).bind(v.name.trim()).bind(v.prompt.trim()).bind(v.agent_id.trim()).bind(v.working_directory.trim()).bind(v.model.as_deref()).bind(&v.schedule_type).bind(v.schedule_config.to_string()).bind(v.timezone.trim()).bind(&v.session_strategy).bind(v.reuse_session_id.as_deref()).bind(v.enabled as i64).bind(if v.enabled {next} else {None}).bind(&now).bind(&now)
        .execute(&s.db).await.map_err(|e| { tracing::error!(%e,"create scheduled task failed"); StatusCode::INTERNAL_SERVER_ERROR })?;
    get(Path(id), State(s)).await
}
pub async fn update(Path(id): Path<String>, State(s): State<AppState>, Json(v): Json<TaskInput>) -> Result<Json<Value>, StatusCode> {
    validate(&v)?;
    crate::api::build_agent_config(&s.db, v.agent_id.trim()).await.map_err(|_| StatusCode::BAD_REQUEST)?;
    if v.session_strategy == "reuse" {
        let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE id=?")
            .bind(v.reuse_session_id.as_deref().unwrap_or(""))
            .fetch_one(&s.db).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if exists == 0 { return Err(StatusCode::BAD_REQUEST); }
    }
    let next = initial_next(&v)?;
    let now = Utc::now().to_rfc3339();
    let result = sqlx::query("UPDATE scheduled_tasks SET name=?,prompt=?,agent_id=?,working_directory=?,model=?,schedule_type=?,schedule_config=?,timezone=?,session_strategy=?,reuse_session_id=?,enabled=?,next_run_at=?,updated_at=? WHERE id=?")
        .bind(v.name.trim()).bind(v.prompt.trim()).bind(v.agent_id.trim()).bind(v.working_directory.trim()).bind(v.model.as_deref()).bind(&v.schedule_type).bind(v.schedule_config.to_string()).bind(v.timezone.trim()).bind(&v.session_strategy).bind(v.reuse_session_id.as_deref()).bind(v.enabled as i64).bind(if v.enabled {next} else {None}).bind(&now).bind(&id).execute(&s.db).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if result.rows_affected()==0 { return Err(StatusCode::NOT_FOUND); }
    get(Path(id), State(s)).await
}
pub async fn delete(Path(id): Path<String>, State(s): State<AppState>) -> Result<StatusCode, StatusCode> {
    let result=sqlx::query("DELETE FROM scheduled_tasks WHERE id=?").bind(id).execute(&s.db).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    if result.rows_affected()==0 { return Err(StatusCode::NOT_FOUND); }
    Ok(StatusCode::NO_CONTENT)
}
async fn set_enabled(s: &AppState, id: &str, enabled: bool) -> Result<(), StatusCode> {
    let now=Utc::now().to_rfc3339();
    let row=sqlx::query("SELECT schedule_type,schedule_config,timezone FROM scheduled_tasks WHERE id=?").bind(id).fetch_optional(&s.db).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?.ok_or(StatusCode::NOT_FOUND)?;
    let kind:String=row.get(0); let config:String=row.get(1); let timezone:String=row.get(2);
    let config:Value=serde_json::from_str(&config).map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    let next=if enabled { if kind=="once" { let at=config.get("at").and_then(Value::as_str).unwrap_or(""); if let Ok(parsed)=chrono::DateTime::parse_from_rfc3339(at) { let parsed=parsed.with_timezone(&Utc); if parsed>Utc::now() {Some(parsed.to_rfc3339_opts(chrono::SecondsFormat::Millis,true))} else {Some(Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis,true))} } else {None} } else {
        // Enabling recurring tasks schedules from now, avoiding a burst of historical runs.
        if kind=="interval" { Some((Utc::now()+chrono::Duration::seconds(config.get("seconds").and_then(Value::as_i64).unwrap_or(60))).to_rfc3339()) }
        else { initial_next(&TaskInput{name:"x".into(),prompt:"x".into(),agent_id:"x".into(),working_directory:".".into(),model:None,schedule_type:kind.clone(),schedule_config:config,timezone,session_strategy:"new".into(),reuse_session_id:None,enabled:true})? }
    }} else {None};
    sqlx::query("UPDATE scheduled_tasks SET enabled=?,next_run_at=?,updated_at=? WHERE id=?").bind(enabled as i64).bind(next).bind(now).bind(id).execute(&s.db).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(())
}
pub async fn enable(Path(id):Path<String>,State(s):State<AppState>)->Result<Json<Value>,StatusCode>{set_enabled(&s,&id,true).await?;get(Path(id),State(s)).await}
pub async fn disable(Path(id):Path<String>,State(s):State<AppState>)->Result<Json<Value>,StatusCode>{set_enabled(&s,&id,false).await?;get(Path(id),State(s)).await}

pub async fn run_now(Path(id):Path<String>,State(s):State<AppState>)->Result<Json<Value>,StatusCode>{
    let row=sqlx::query("SELECT name FROM scheduled_tasks WHERE id=?").bind(&id).fetch_optional(&s.db).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?.ok_or(StatusCode::NOT_FOUND)?;
    let name:String=row.get(0); let now=Utc::now().to_rfc3339(); let run_id=Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO scheduled_task_runs(id,task_id,task_name,status,scheduled_at,created_at,updated_at) VALUES(?,?,?,'queued',?,?,?)")
        .bind(&run_id).bind(id).bind(name).bind(&now).bind(&now).bind(&now).execute(&s.db).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({"run_id":run_id,"status":"queued"})))
}
#[derive(Deserialize)] pub struct HistoryQuery { pub limit: Option<i64>, pub offset: Option<i64> }
pub async fn history(Path(id):Path<String>,Query(q):Query<HistoryQuery>,State(s):State<AppState>)->Result<Json<Vec<Value>>,StatusCode>{
    let exists:i64=sqlx::query_scalar("SELECT COUNT(*) FROM scheduled_tasks WHERE id=?").bind(&id).fetch_one(&s.db).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    if exists==0{return Err(StatusCode::NOT_FOUND)}
    let limit=q.limit.unwrap_or(50).clamp(1,200);let offset=q.offset.unwrap_or(0).max(0);
    let rows=sqlx::query("SELECT id,task_id,task_name,status,scheduled_at,started_at,finished_at,agent_session_id,summary,error_message,created_at,updated_at FROM scheduled_task_runs WHERE task_id=? ORDER BY created_at DESC LIMIT ? OFFSET ?").bind(id).bind(limit).bind(offset).fetch_all(&s.db).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(rows.iter().map(|r|json!({"id":r.get::<String,_>("id"),"task_id":r.get::<String,_>("task_id"),"task_name":r.get::<String,_>("task_name"),"status":r.get::<String,_>("status"),"scheduled_at":r.get::<String,_>("scheduled_at"),"started_at":r.get::<Option<String>,_>("started_at"),"finished_at":r.get::<Option<String>,_>("finished_at"),"agent_session_id":r.get::<Option<String>,_>("agent_session_id"),"summary":r.get::<Option<String>,_>("summary"),"error_message":r.get::<Option<String>,_>("error_message"),"created_at":r.get::<String,_>("created_at"),"updated_at":r.get::<String,_>("updated_at")})).collect()))
}
