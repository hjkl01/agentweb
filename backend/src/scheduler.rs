use crate::{agents::process::run_session, api::Session, state::AppState};
use chrono::{DateTime, Datelike, Duration, SecondsFormat, Timelike, Utc};
use serde_json::Value;
use sqlx::{Row, SqlitePool};
use std::time::Duration as StdDuration;
use tokio::time;
use uuid::Uuid;

const POLL_SECONDS: u64 = 2;
const LEASE_SECONDS: i64 = 30;

fn stamp(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn session_lease_is_live(status: &str, lease_until: Option<&str>, now: DateTime<Utc>) -> bool {
    if status != "running" {
        return false;
    }
    let Some(lease) = lease_until else {
        return true;
    };
    match DateTime::parse_from_rfc3339(lease) {
        Ok(lease) => lease.with_timezone(&Utc) >= now,
        // Unknown lease state must be handled conservatively to avoid duplicate execution.
        Err(_) => true,
    }
}

fn cron_field_matches(field: &str, value: u32, min: u32, max: u32) -> bool {
    field.split(',').any(|part| {
        let mut step_split = part.splitn(2, '/');
        let base = step_split.next().unwrap_or("");
        let step = step_split
            .next()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(1);
        if step == 0 {
            return false;
        }
        let (lo, hi) = if base == "*" {
            (min, max)
        } else if let Some((a, b)) = base.split_once('-') {
            match (a.parse::<u32>(), b.parse::<u32>()) {
                (Ok(a), Ok(b)) => (a, b),
                _ => return false,
            }
        } else {
            match base.parse::<u32>() {
                Ok(v) if part.contains('/') => (v, max),
                Ok(v) => (v, v),
                Err(_) => return false,
            }
        };
        value >= lo && value <= hi && (value - lo) % step == 0
    })
}

fn next_cron(config: &Value, after: DateTime<Utc>, timezone: &str) -> Option<DateTime<Utc>> {
    let expr = config.get("expression")?.as_str()?;
    let fields: Vec<_> = expr.split_whitespace().collect();
    if fields.len() != 5 {
        return None;
    }
    // Cron expressions are evaluated in UTC unless a timezone-aware expression is added later.
    // Reject non-UTC zones instead of silently scheduling at the wrong local time.
    if timezone != "UTC" && timezone != "Etc/UTC" {
        return None;
    }
    let mut candidate = after.with_second(0)?.with_nanosecond(0)? + Duration::minutes(1);
    for _ in 0..60 * 24 * 366 {
        let weekday = candidate.weekday().num_days_from_sunday();
        if cron_field_matches(fields[0], candidate.minute(), 0, 59)
            && cron_field_matches(fields[1], candidate.hour(), 0, 23)
            && cron_field_matches(fields[2], candidate.day(), 1, 31)
            && cron_field_matches(fields[3], candidate.month(), 1, 12)
            && cron_field_matches(fields[4], weekday, 0, 6)
        {
            return Some(candidate);
        }
        candidate += Duration::minutes(1);
    }
    None
}

fn next_occurrence(
    kind: &str,
    config: &Value,
    timezone: &str,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    match kind {
        "once" => None,
        "interval" => {
            let seconds = config
                .get("seconds")
                .and_then(Value::as_i64)
                .filter(|v| *v >= 1)?;
            Some(now + Duration::seconds(seconds))
        }
        "cron" => next_cron(config, now, timezone),
        _ => None,
    }
}

async fn materialize_due(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let now = Utc::now();
    let now_text = stamp(now);
    let rows = sqlx::query("SELECT id,name,prompt,agent_id,working_directory,model,schedule_type,schedule_config,timezone,next_run_at FROM scheduled_tasks WHERE enabled=1 AND next_run_at IS NOT NULL AND julianday(next_run_at)<=julianday(?) ORDER BY julianday(next_run_at),next_run_at LIMIT 100")
        .bind(&now_text).fetch_all(pool).await?;
    for row in rows {
        let id: String = row.get("id");
        let name: String = row.get("name");
        let prompt: String = row.get("prompt");
        let agent_id: String = row.get("agent_id");
        let workspace: String = row.get("working_directory");
        let model: Option<String> = row.get("model");
        let kind: String = row.get("schedule_type");
        let raw_config: String = row.get("schedule_config");
        let timezone: String = row.get("timezone");
        let scheduled_at: String = row.get("next_run_at");
        let config: Value = match serde_json::from_str(&raw_config) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let next = next_occurrence(&kind, &config, &timezone, now).map(stamp);
        let run_id = Uuid::new_v4().to_string();
        let mut tx = pool.begin().await?;
        // Compare-and-swap prevents two pollers advancing the same due occurrence.
        let changed = sqlx::query("UPDATE scheduled_tasks SET next_run_at=?,last_run_at=?,enabled=CASE WHEN ?='once' THEN 0 ELSE enabled END,updated_at=? WHERE id=? AND enabled=1 AND next_run_at=?")
            .bind(&next).bind(&scheduled_at).bind(&kind).bind(&now_text).bind(&id).bind(&scheduled_at).execute(&mut *tx).await?;
        if changed.rows_affected() == 1 {
            // Coalesce an occurrence if this task already has queued/running work.
            // Otherwise a short interval could enqueue overlapping executions while
            // an earlier run is still active. Keep the schedule cursor moving so
            // downtime/long runs do not produce a backlog.
            sqlx::query("INSERT INTO scheduled_task_runs(id,task_id,task_name,status,scheduled_at,created_at,updated_at) SELECT ?,?,?, 'queued',?,?,? WHERE NOT EXISTS (SELECT 1 FROM scheduled_task_runs WHERE task_id=? AND status IN ('queued','running'))")
                .bind(run_id).bind(&id).bind(&name).bind(&scheduled_at).bind(&now_text).bind(&now_text).bind(&id)
                .execute(&mut *tx).await?;
        }
        tx.commit().await?;
        // Config is validated at API creation time; malformed legacy rows are skipped safely.
        let _ = (prompt, agent_id, workspace, model);
    }
    Ok(())
}

async fn recover_expired(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let now = stamp(Utc::now());
    sqlx::query("UPDATE scheduled_task_runs SET status='failed',finished_at=?,error_message=COALESCE(error_message,'parent task missing'),lease_owner=NULL,lease_until=NULL,updated_at=? WHERE status='queued' AND NOT EXISTS (SELECT 1 FROM scheduled_tasks t WHERE t.id=scheduled_task_runs.task_id)")
        .bind(&now).bind(&now).execute(pool).await?;
    // Do not requeue a run while its associated session still has a live lease.
    // This closes the small recovery race where the run lease expires first and a
    // second worker would otherwise claim the run while the old Agent still owns
    // the session.
    sqlx::query("UPDATE scheduled_task_runs SET status='queued',lease_owner=NULL,lease_until=NULL,updated_at=? WHERE status='running' AND (lease_until IS NULL OR julianday(lease_until) IS NULL OR julianday(lease_until)<julianday(?)) AND (agent_session_id IS NULL OR NOT EXISTS (SELECT 1 FROM session_runtime s WHERE s.session_id=scheduled_task_runs.agent_session_id AND s.status='running' AND (s.lease_until IS NULL OR julianday(s.lease_until) IS NULL OR julianday(s.lease_until)>=julianday(?))))")
        .bind(&now).bind(&now).bind(&now).execute(pool).await?;
    Ok(())
}

async fn claim(pool: &SqlitePool, worker: &str) -> Result<Option<String>, sqlx::Error> {
    let now = Utc::now();
    let now_text = stamp(now);
    let lease = stamp(now + Duration::seconds(LEASE_SECONDS));
    // One UPDATE ... RETURNING is the linearization point for concurrent workers.
    sqlx::query_scalar::<_, String>(
        "UPDATE scheduled_task_runs SET status='running',lease_owner=?,lease_until=?,started_at=COALESCE(started_at,?),updated_at=? WHERE id=(SELECT id FROM scheduled_task_runs WHERE status='queued' ORDER BY scheduled_at,created_at LIMIT 1) AND status='queued' RETURNING id"
    )
        .bind(worker).bind(lease).bind(&now_text).bind(&now_text)
        .fetch_optional(pool).await
}

async fn renew_run_lease_once(pool: &SqlitePool, run_id: &str, worker: &str) -> bool {
    let now = Utc::now();
    let now_text = stamp(now);
    let lease = stamp(now + Duration::seconds(LEASE_SECONDS));
    matches!(sqlx::query("UPDATE scheduled_task_runs SET lease_until=?,updated_at=? WHERE id=? AND status='running' AND lease_owner=?")
        .bind(lease).bind(now_text).bind(run_id).bind(worker).execute(pool).await, Ok(r) if r.rows_affected() == 1)
}

async fn renew_session_lease_once(pool: &SqlitePool, session_id: &str, worker: &str) -> bool {
    let now = Utc::now();
    let now_text = stamp(now);
    let lease = stamp(now + Duration::seconds(LEASE_SECONDS));
    matches!(sqlx::query("UPDATE session_runtime SET lease_until=?,updated_at=? WHERE session_id=? AND status='running' AND worker_id=?")
        .bind(lease).bind(now_text).bind(session_id).bind(worker).execute(pool).await, Ok(r) if r.rows_affected() == 1)
}

async fn renew_run_lease(
    pool: SqlitePool,
    run_id: String,
    session_id: String,
    run_worker: String,
    session_worker: String,
    lease_lost: tokio::sync::oneshot::Sender<()>,
) {
    let mut ticker = time::interval(StdDuration::from_secs((LEASE_SECONDS as u64 / 3).max(1)));
    loop {
        ticker.tick().await;
        // Renew both leases together. If either lease is lost, notify the caller so
        // it can interrupt the Agent rather than letting an unowned task continue.
        if !renew_run_lease_once(&pool, &run_id, &run_worker).await
            || !renew_session_lease_once(&pool, &session_id, &session_worker).await
        {
            tracing::warn!(%run_id, %session_id, run_worker = %run_worker, session_worker = %session_worker, "scheduled task lost its run or session lease");
            let _ = lease_lost.send(());
            return;
        }
    }
}

async fn execute_run(state: AppState, run_id: &str) {
    // Revalidate ownership before doing any setup work. A worker may have been
    // delayed after claim while another worker recovered and reclaimed this run.
    let row = match sqlx::query("SELECT r.task_id,t.prompt,t.agent_id,t.working_directory,t.model,t.session_strategy,t.reuse_session_id FROM scheduled_task_runs r JOIN scheduled_tasks t ON t.id=r.task_id WHERE r.id=? AND r.status='running' AND r.lease_owner=?")
        .bind(run_id).bind(&state.worker_id).fetch_optional(&state.db).await {
        Ok(Some(row)) => row,
        // No row means this worker no longer owns the run (or it was removed).
        // Never mutate the run here: it may already belong to another worker.
        Ok(None) => return,
        Err(error) => {
            tracing::error!(%run_id, %error, "failed to load task for claimed run");
            finish_run(&state.db, run_id, &state.worker_id, "failed", Some("读取任务配置失败")).await;
            return;
        }
    };
    let task_id: String = row.get(0);
    let prompt: String = row.get(1);
    let agent_id: String = row.get(2);
    let workspace: String = row.get(3);
    let model: Option<String> = row.get(4);
    let strategy: String = row.get(5);
    let reuse_id: Option<String> = row.get(6);
    let now = Utc::now();
    let now_text = stamp(now);
    let session_id = if strategy == "reuse" {
        match reuse_id {
            Some(id) => id,
            None => {
                finish_run(
                    &state.db,
                    run_id,
                    &state.worker_id,
                    "failed",
                    Some("复用会话 ID 缺失"),
                )
                .await;
                return;
            }
        }
    } else {
        Uuid::new_v4().to_string()
    };
    let session_row = sqlx::query("SELECT s.id,s.agent_id,s.title,s.workspace,r.status,s.native_session_id,s.model,s.is_pinned,r.lease_until,s.fork_from_native_session_id FROM sessions s JOIN session_runtime r ON r.session_id=s.id WHERE s.id=?")
        .bind(&session_id).fetch_optional(&state.db).await;
    let session = match session_row {
        Ok(Some(r)) if strategy == "reuse" => {
            let status: String = r.get(4);
            let session_lease: Option<String> = r.get(8);
            if session_lease_is_live(&status, session_lease.as_deref(), Utc::now()) {
                finish_run(
                    &state.db,
                    run_id,
                    &state.worker_id,
                    "failed",
                    Some("目标会话正在执行或租约状态未知"),
                )
                .await;
                return;
            }
            Session {
                id: r.get(0),
                agent_id: r.get(1),
                title: r.get(2),
                workspace: r.get(3),
                status,
                native_session_id: r.get(5),
                model: r.get(6),
                is_pinned: r.get::<i64, _>(7) != 0,
                fork_from_native_session_id: r.get(9),
                workspace_available: true,
            }
        }
        Ok(None) if strategy == "new" => {
            if crate::api::build_agent_config(&state.db, &agent_id)
                .await
                .is_err()
            {
                finish_run(
                    &state.db,
                    run_id,
                    &state.worker_id,
                    "failed",
                    Some("Agent 未配置或不可用"),
                )
                .await;
                return;
            }
            let created = async {
                let mut tx = state.db.begin().await?;
                sqlx::query("INSERT INTO sessions(id,agent_id,title,workspace,status,native_session_id,model,is_pinned,created_at,updated_at) VALUES(?,?,?,?,'idle',NULL,?,0,?,?)")
                    .bind(&session_id).bind(&agent_id).bind(format!("定时任务：{}", task_id)).bind(&workspace).bind(&model).bind(&now_text).bind(&now_text).execute(&mut *tx).await?;
                sqlx::query("INSERT INTO session_runtime(session_id,status,worker_id,lease_until,updated_at) VALUES(?,'idle',NULL,NULL,?)")
                    .bind(&session_id).bind(&now_text).execute(&mut *tx).await?;
                tx.commit().await
            }.await;
            if created.is_err() {
                finish_run(&state.db, run_id, &state.worker_id, "failed", Some("创建执行会话失败")).await; return;
            }
            Session {
                id: session_id.clone(),
                agent_id,
                title: format!("定时任务：{}", task_id),
                workspace,
                status: "idle".into(),
                native_session_id: None,
                model,
                is_pinned: false,
                fork_from_native_session_id: None,
                workspace_available: true,
            }
        }
        Ok(None) => {
            finish_run(
                &state.db,
                run_id,
                &state.worker_id,
                "failed",
                Some("复用的会话不存在"),
            )
            .await;
            return;
        }
        Ok(Some(_)) => {
            finish_run(
                &state.db,
                run_id,
                &state.worker_id,
                "failed",
                Some("新会话 ID 冲突"),
            )
            .await;
            return;
        }
        Err(_) => {
            finish_run(
                &state.db,
                run_id,
                &state.worker_id,
                "failed",
                Some("读取会话失败"),
            )
            .await;
            return;
        }
    };
    // Link the session before claiming it. If the process crashes immediately
    // after the session claim, recovery can still see the association and will
    // respect a live session lease instead of requeueing a duplicate run.
    let linked = sqlx::query("UPDATE scheduled_task_runs SET agent_session_id=?,updated_at=? WHERE id=? AND status='running' AND lease_owner=?")
        .bind(&session.id).bind(&now_text).bind(run_id).bind(&state.worker_id).execute(&state.db).await;
    if !matches!(linked, Ok(ref result) if result.rows_affected() == 1) {
        tracing::warn!(%run_id, session_id = %session.id, "run ownership changed before session claim; Agent will not start");
        return;
    }
    let lease = stamp(now + Duration::seconds(LEASE_SECONDS));
    let session_worker_id = format!("{}:{}", state.worker_id, Uuid::new_v4());
    let claimed = sqlx::query("UPDATE session_runtime SET status='running',worker_id=?,lease_until=?,updated_at=? WHERE session_id=? AND status!='missing' AND (status!='running' OR (lease_until IS NOT NULL AND julianday(lease_until) IS NOT NULL AND julianday(lease_until)<julianday(?)))")
        .bind(&session_worker_id).bind(&lease).bind(&now_text).bind(&session.id).bind(&now_text).execute(&state.db).await;
    if !matches!(claimed, Ok(ref r) if r.rows_affected()==1) {
        finish_run(
            &state.db,
            run_id,
            &state.worker_id,
            "failed",
            Some("会话无法认领，可能正在运行"),
        )
        .await;
        return;
    }
    // The run lease may have expired after the initial link but before the
    // session claim. Re-check ownership before starting any Agent process.
    let still_owned: i64 = match sqlx::query_scalar("SELECT COUNT(*) FROM scheduled_task_runs WHERE id=? AND status='running' AND lease_owner=?")
        .bind(run_id).bind(&state.worker_id).fetch_one(&state.db).await {
        Ok(count) => count,
        Err(error) => {
            tracing::error!(%run_id, %error, "failed to revalidate scheduled run ownership");
            0
        }
    };
    if still_owned != 1 {
        let release_time = stamp(Utc::now());
        let _ = sqlx::query("UPDATE session_runtime SET status='idle',worker_id=NULL,lease_until=NULL,updated_at=? WHERE session_id=? AND status='running' AND worker_id=?")
            .bind(&release_time).bind(&session.id).bind(&session_worker_id).execute(&state.db).await;
        tracing::warn!(%run_id, session_id = %session.id, "run lease was lost before Agent startup; released session");
        return;
    }
    let renew_db = state.db.clone();
    let renew_run = run_id.to_owned();
    let renew_session = session.id.clone();
    let renew_worker = state.worker_id.clone();
    let renew_session_worker = session_worker_id.clone();
    let (lease_lost_tx, mut lease_lost_rx) = tokio::sync::oneshot::channel();
    let heartbeat = tokio::spawn(async move {
        renew_run_lease(
            renew_db,
            renew_run,
            renew_session,
            renew_worker,
            renew_session_worker,
            lease_lost_tx,
        )
        .await;
    });
    let session_id_for_run = session.id.clone();
    let agent_kind_for_run: String = sqlx::query_scalar("SELECT kind FROM agents WHERE id=?")
        .bind(&session.agent_id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| session.agent_id.clone());
    let run_future = run_session(
        state.db.clone(),
        state.agents.clone(),
        session,
        prompt,
        state.events.clone(),
        session_worker_id.clone(),
    );
    tokio::pin!(run_future);
    tokio::select! {
        _ = &mut run_future => {}
        _ = &mut lease_lost_rx => {
            tracing::error!(%run_id, session_id = %session_id_for_run, "interrupting scheduled Agent because its lease was lost");
            if let Err(error) = state.agents.adapter(&agent_kind_for_run).await.interrupt(&session_id_for_run, &session_worker_id).await {
                tracing::error!(%run_id, session_id = %session_id_for_run, %error, "failed to interrupt scheduled Agent after lease loss");
            }
            // The run lease is no longer ours, but the Agent may still be shutting
            // down (or interrupt may have failed). Keep renewing the session lease
            // while run_session is alive so recovery cannot launch a duplicate run
            // merely because the old session lease timed out.
            let mut session_guard = time::interval(StdDuration::from_secs((LEASE_SECONDS as u64 / 3).max(1)));
            loop {
                tokio::select! {
                    _ = &mut run_future => break,
                    _ = session_guard.tick() => {
                        if !renew_session_lease_once(&state.db, &session_id_for_run, &session_worker_id).await {
                            tracing::warn!(%run_id, session_id = %session_id_for_run, "could not renew session lease while waiting for Agent shutdown");
                        }
                    }
                }
            }
        }
    }
    heartbeat.abort();
    let status_row = sqlx::query("SELECT status,error_message FROM session_runtime WHERE session_id=?")
        .bind(&session_id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();
    let status = status_row.as_ref().map(|row| row.get::<String, _>("status"));
    let runtime_error = status_row.as_ref().and_then(|row| row.get::<Option<String>, _>("error_message"));
    let (final_status, error) = match status.as_deref() {
        Some("idle") => ("succeeded", None),
        Some("interrupted") => ("cancelled", Some("执行被中断".to_owned())),
        Some("error" | "missing") => ("failed", Some(runtime_error.unwrap_or_else(|| "Agent 执行失败".to_owned()))),
        _ => ("failed", Some("执行结束后会话状态异常".to_owned())),
    };
    finish_run(&state.db, run_id, &state.worker_id, final_status, error.as_deref()).await;
}

async fn finish_run(pool: &SqlitePool, id: &str, worker: &str, status: &str, error: Option<&str>) {
    let now = stamp(Utc::now());
    let _ = sqlx::query("UPDATE scheduled_task_runs SET status=?,finished_at=?,summary=CASE WHEN ?='succeeded' THEN '执行完成' ELSE summary END,error_message=?,lease_owner=NULL,lease_until=NULL,updated_at=? WHERE id=? AND status='running' AND lease_owner=?")
        .bind(status).bind(&now).bind(status).bind(error).bind(&now).bind(id).bind(worker).execute(pool).await;
}

pub fn start(state: AppState) {
    tokio::spawn(async move {
        let mut ticker = time::interval(StdDuration::from_secs(POLL_SECONDS));
        loop {
            ticker.tick().await;
            if let Err(error) = recover_expired(&state.db).await {
                tracing::warn!(%error, "scheduled task recovery failed");
                continue;
            }
            if let Err(error) = materialize_due(&state.db).await {
                tracing::warn!(%error, "scheduled task scan failed");
                continue;
            }
            match claim(&state.db, &state.worker_id).await {
                Ok(Some(id)) => {
                    let state = state.clone();
                    tokio::spawn(async move {
                        execute_run(state, &id).await;
                    });
                }
                Ok(None) => {}
                Err(error) => tracing::warn!(%error, "scheduled task claim failed"),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expired_reused_session_lease_can_be_reclaimed() {
        let now = Utc::now();
        let expired = stamp(now - Duration::seconds(1));
        assert!(!session_lease_is_live("running", Some(&expired), now));
    }

    #[test]
    fn live_or_unknown_reused_session_lease_is_not_reclaimed() {
        let now = Utc::now();
        let live = stamp(now + Duration::seconds(30));
        assert!(session_lease_is_live("running", Some(&live), now));
        assert!(session_lease_is_live("running", None, now));
        assert!(session_lease_is_live("running", Some("invalid"), now));
        assert!(!session_lease_is_live("idle", None, now));
    }

    #[test]
    fn interval_schedule_skips_missed_occurrences_in_one_step() {
        let now = Utc::now();
        let next =
            next_occurrence("interval", &serde_json::json!({"seconds":60}), "UTC", now).unwrap();
        assert!(next > now && next <= now + Duration::seconds(61));
    }
    #[test]
    fn cron_expression_finds_next_minute() {
        let now = Utc::now();
        let next = next_cron(&serde_json::json!({"expression":"* * * * *"}), now, "UTC").unwrap();
        assert!(next > now && next <= now + Duration::minutes(1));
    }
    #[test]
    fn cron_single_value_step_advances_through_field_range() {
        let after = DateTime::parse_from_rfc3339("2026-01-01T00:05:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let next = next_cron(
            &serde_json::json!({"expression":"5/10 * * * *"}),
            after,
            "UTC",
        )
        .unwrap();
        assert_eq!(next.to_rfc3339(), "2026-01-01T00:15:00+00:00");
    }

    #[test]
    fn invalid_cron_is_rejected() {
        assert!(next_cron(
            &serde_json::json!({"expression":"not cron"}),
            Utc::now(),
            "UTC"
        )
        .is_none());
    }
    async fn test_pool() -> SqlitePool {
        let path = std::env::temp_dir().join(format!("agentweb-scheduler-{}.db", Uuid::new_v4()));
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&url)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE scheduled_task_runs(id TEXT PRIMARY KEY, task_id TEXT NOT NULL, task_name TEXT NOT NULL, status TEXT NOT NULL, scheduled_at TEXT NOT NULL, started_at TEXT, finished_at TEXT, agent_session_id TEXT, summary TEXT, error_message TEXT, lease_owner TEXT, lease_until TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL)").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE sessions(id TEXT PRIMARY KEY, status TEXT NOT NULL DEFAULT 'idle', worker_id TEXT, lease_until TEXT, updated_at TEXT NOT NULL DEFAULT '')").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE session_runtime(session_id TEXT PRIMARY KEY, status TEXT NOT NULL DEFAULT 'idle', worker_id TEXT, lease_until TEXT, updated_at TEXT NOT NULL DEFAULT '')").execute(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn two_workers_cannot_claim_the_same_run() {
        let pool = test_pool().await;
        let now = stamp(Utc::now());
        sqlx::query("INSERT INTO scheduled_task_runs(id,task_id,task_name,status,scheduled_at,created_at,updated_at) VALUES('r1','t1','task','queued',?,?,?)")
            .bind(&now).bind(&now).bind(&now).execute(&pool).await.unwrap();
        let p1 = pool.clone();
        let p2 = pool.clone();
        let (a, b) = tokio::join!(claim(&p1, "worker-a"), claim(&p2, "worker-b"));
        let claimed = [a.unwrap(), b.unwrap()].into_iter().flatten().count();
        assert_eq!(claimed, 1, "only one worker may claim a queued run");
    }

    #[tokio::test]
    async fn expired_running_lease_is_requeued_after_worker_crash() {
        let pool = test_pool().await;
        let old = stamp(Utc::now() - Duration::minutes(1));
        sqlx::query("INSERT INTO scheduled_task_runs(id,task_id,task_name,status,scheduled_at,lease_owner,lease_until,created_at,updated_at) VALUES('r1','t1','task','running',?,'dead-worker',?,?,?)")
            .bind(&old).bind(&old).bind(&old).bind(&old).execute(&pool).await.unwrap();
        recover_expired(&pool).await.unwrap();
        let status: String =
            sqlx::query_scalar("SELECT status FROM scheduled_task_runs WHERE id='r1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status, "queued");
    }
    #[tokio::test]
    async fn expired_run_waits_for_live_session_lease_before_requeueing() {
        let pool = test_pool().await;
        let now = Utc::now();
        let expired = stamp(now - Duration::minutes(1));
        let live = stamp(now + Duration::minutes(1));
        sqlx::query("INSERT INTO sessions(id,status,lease_until) VALUES('s1','running',?)")
            .bind(&live)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO session_runtime(session_id,status,lease_until,updated_at) VALUES('s1','running',?,?)")
            .bind(&live).bind(&expired).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO scheduled_task_runs(id,task_id,task_name,status,scheduled_at,agent_session_id,lease_owner,lease_until,created_at,updated_at) VALUES('r1','t1','task','running',?,'s1','worker-a',?,?,?)")
            .bind(&expired).bind(&expired).bind(&expired).bind(&expired).execute(&pool).await.unwrap();

        recover_expired(&pool).await.unwrap();
        let status: String =
            sqlx::query_scalar("SELECT status FROM scheduled_task_runs WHERE id='r1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            status, "running",
            "must not requeue while the associated session lease is live"
        );

        sqlx::query("UPDATE session_runtime SET lease_until=?,updated_at=? WHERE session_id='s1'")
            .bind(&expired).bind(&expired).execute(&pool).await.unwrap();
        recover_expired(&pool).await.unwrap();
        let status: String =
            sqlx::query_scalar("SELECT status FROM scheduled_task_runs WHERE id='r1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            status, "queued",
            "must requeue after the associated session lease expires"
        );
    }

    #[tokio::test]
    async fn expired_run_does_not_requeue_when_running_session_has_no_lease() {
        let pool = test_pool().await;
        let old = stamp(Utc::now() - Duration::minutes(1));
        sqlx::query(
            "INSERT INTO sessions(id,status,lease_until) VALUES('s-no-lease','running',NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO session_runtime(session_id,status,lease_until,updated_at) VALUES('s-no-lease','running',NULL,?)")
            .bind(&old).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO scheduled_task_runs(id,task_id,task_name,status,scheduled_at,agent_session_id,lease_owner,lease_until,created_at,updated_at) VALUES('r-no-lease','t1','task','running',?,'s-no-lease','dead-worker',?,?,?)")
            .bind(&old).bind(&old).bind(&old).bind(&old).execute(&pool).await.unwrap();

        recover_expired(&pool).await.unwrap();

        let status: String =
            sqlx::query_scalar("SELECT status FROM scheduled_task_runs WHERE id='r-no-lease'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            status, "running",
            "an unknown session lease must not trigger a possible duplicate execution"
        );
    }

    #[tokio::test]
    async fn expired_run_does_not_requeue_when_session_lease_is_malformed() {
        let pool = test_pool().await;
        let old = stamp(Utc::now() - Duration::minutes(1));
        sqlx::query("INSERT INTO sessions(id,status,lease_until) VALUES('s-bad-lease','running','bad-timestamp')")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO session_runtime(session_id,status,lease_until,updated_at) VALUES('s-bad-lease','running','bad-timestamp',?)")
            .bind(&old).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO scheduled_task_runs(id,task_id,task_name,status,scheduled_at,agent_session_id,lease_owner,lease_until,created_at,updated_at) VALUES('r-bad-lease','t1','task','running',?,'s-bad-lease','dead-worker',?,?,?)")
            .bind(&old).bind(&old).bind(&old).bind(&old).execute(&pool).await.unwrap();

        recover_expired(&pool).await.unwrap();

        let status: String =
            sqlx::query_scalar("SELECT status FROM scheduled_task_runs WHERE id='r-bad-lease'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            status, "running",
            "a malformed session lease must be treated conservatively"
        );
    }

    #[tokio::test]
    async fn running_worker_can_renew_lease_but_other_worker_cannot() {
        let pool = test_pool().await;
        let old = stamp(Utc::now() - Duration::seconds(1));
        sqlx::query("INSERT INTO scheduled_task_runs(id,task_id,task_name,status,scheduled_at,lease_owner,lease_until,created_at,updated_at) VALUES('r1','t1','task','running',?,'worker-a',?,?,?)")
            .bind(&old).bind(&old).bind(&old).bind(&old).execute(&pool).await.unwrap();
        assert!(renew_run_lease_once(&pool, "r1", "worker-a").await);
        assert!(!renew_run_lease_once(&pool, "r1", "worker-b").await);
    }

    #[tokio::test]
    async fn session_lease_is_renewed_only_by_its_owner() {
        let pool = test_pool().await;
        let old = stamp(Utc::now() - Duration::seconds(1));
        sqlx::query("INSERT INTO sessions(id,status,worker_id,lease_until,updated_at) VALUES('s1','running','worker-a',?,?)")
            .bind(&old).bind(&old).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO session_runtime(session_id,status,worker_id,lease_until,updated_at) VALUES('s1','running','worker-a',?,?)")
            .bind(&old).bind(&old).execute(&pool).await.unwrap();

        assert!(renew_session_lease_once(&pool, "s1", "worker-a").await);
        assert!(!renew_session_lease_once(&pool, "s1", "worker-b").await);

        let lease: String = sqlx::query_scalar("SELECT lease_until FROM session_runtime WHERE session_id='s1'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(
            DateTime::parse_from_rfc3339(&lease)
                .unwrap()
                .with_timezone(&Utc)
                > Utc::now()
        );
    }

    #[tokio::test]
    async fn many_missed_interval_occurrences_coalesce_to_one_run() {
        let pool = test_pool().await;
        sqlx::query("CREATE TABLE scheduled_tasks(id TEXT PRIMARY KEY,name TEXT,prompt TEXT,agent_id TEXT,working_directory TEXT,model TEXT,schedule_type TEXT,schedule_config TEXT,timezone TEXT,next_run_at TEXT,enabled INTEGER,last_run_at TEXT,updated_at TEXT)")
            .execute(&pool).await.unwrap();
        let old = stamp(Utc::now() - Duration::hours(4));
        sqlx::query("INSERT INTO scheduled_tasks(id,name,prompt,agent_id,working_directory,schedule_type,schedule_config,timezone,next_run_at,enabled,updated_at) VALUES('t1','task','do it','codex','/tmp','interval','{\"seconds\":60}','UTC',?,1,?)")
            .bind(&old).bind(&old).execute(&pool).await.unwrap();
        materialize_due(&pool).await.unwrap();
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM scheduled_task_runs WHERE task_id='t1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let next: String =
            sqlx::query_scalar("SELECT next_run_at FROM scheduled_tasks WHERE id='t1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            count, 1,
            "downtime must not create a backlog of missed interval runs"
        );
        assert!(
            chrono::DateTime::parse_from_rfc3339(&next)
                .unwrap()
                .with_timezone(&Utc)
                > Utc::now()
        );
    }

    #[tokio::test]
    async fn due_occurrence_is_coalesced_while_task_has_active_run() {
        let pool = test_pool().await;
        sqlx::query("CREATE TABLE scheduled_tasks(id TEXT PRIMARY KEY,name TEXT,prompt TEXT,agent_id TEXT,working_directory TEXT,model TEXT,schedule_type TEXT,schedule_config TEXT,timezone TEXT,next_run_at TEXT,enabled INTEGER,last_run_at TEXT,updated_at TEXT)")
            .execute(&pool).await.unwrap();
        let old = stamp(Utc::now() - Duration::minutes(5));
        sqlx::query(r#"INSERT INTO scheduled_tasks(id,name,prompt,agent_id,working_directory,schedule_type,schedule_config,timezone,next_run_at,enabled,updated_at) VALUES('t-active','task','do it','codex','/tmp','interval','{"seconds":60}','UTC',?,1,?)"#)
            .bind(&old).bind(&old).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO scheduled_task_runs(id,task_id,task_name,status,scheduled_at,created_at,updated_at) VALUES('existing','t-active','task','running',?,?,?)")
            .bind(&old).bind(&old).bind(&old).execute(&pool).await.unwrap();

        materialize_due(&pool).await.unwrap();

        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM scheduled_task_runs WHERE task_id='t-active'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let next: String =
            sqlx::query_scalar("SELECT next_run_at FROM scheduled_tasks WHERE id='t-active'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 1, "a due occurrence must not overlap an active run");
        assert!(
            DateTime::parse_from_rfc3339(&next)
                .unwrap()
                .with_timezone(&Utc)
                > Utc::now(),
            "the schedule cursor should advance to coalesce missed occurrences"
        );
    }

    #[tokio::test]
    async fn due_schedule_comparison_handles_timezone_offsets() {
        let pool = test_pool().await;
        sqlx::query("CREATE TABLE scheduled_tasks(id TEXT PRIMARY KEY,name TEXT,prompt TEXT,agent_id TEXT,working_directory TEXT,model TEXT,schedule_type TEXT,schedule_config TEXT,timezone TEXT,next_run_at TEXT,enabled INTEGER,last_run_at TEXT,updated_at TEXT)")
            .execute(&pool).await.unwrap();
        // This local timestamp is earlier than UTC now, even though its hour
        // component can sort after a UTC timestamp when compared as plain text.
        let now = Utc::now();
        let due = (now - Duration::minutes(5))
            .with_timezone(&chrono::FixedOffset::east_opt(2 * 3600).unwrap())
            .to_rfc3339();
        sqlx::query("INSERT INTO scheduled_tasks(id,name,prompt,agent_id,working_directory,schedule_type,schedule_config,timezone,next_run_at,enabled,updated_at) VALUES('t-offset','task','do it','codex','/tmp','interval','{\"seconds\":60}','UTC',?,1,?)")
            .bind(&due).bind(stamp(now)).execute(&pool).await.unwrap();

        materialize_due(&pool).await.unwrap();

        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM scheduled_task_runs WHERE task_id='t-offset'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            count, 1,
            "due timestamps must be compared by instant, not lexical representation"
        );
    }

    #[tokio::test]
    async fn orphaned_queued_run_is_failed_without_touching_valid_queue() {
        let pool = test_pool().await;
        sqlx::query("CREATE TABLE scheduled_tasks(id TEXT PRIMARY KEY)")
            .execute(&pool)
            .await
            .unwrap();
        let now = stamp(Utc::now());
        sqlx::query("INSERT INTO scheduled_tasks(id) VALUES('existing-task')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO scheduled_task_runs(id,task_id,task_name,status,scheduled_at,created_at,updated_at) VALUES('orphan','deleted-task','deleted','queued',?,?,?),('valid','existing-task','valid','queued',?,?,?)")
            .bind(&now).bind(&now).bind(&now)
            .bind(&now).bind(&now).bind(&now)
            .execute(&pool).await.unwrap();

        recover_expired(&pool).await.unwrap();

        let orphan: (String, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT status,error_message,lease_owner FROM scheduled_task_runs WHERE id='orphan'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(orphan.0, "failed");
        assert!(orphan
            .1
            .as_deref()
            .is_some_and(|message| message.contains("parent task missing")));
        assert_eq!(orphan.2, None);
        let valid: String =
            sqlx::query_scalar("SELECT status FROM scheduled_task_runs WHERE id='valid'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(valid, "queued", "valid tasks must remain queued");
    }

    #[tokio::test]
    async fn malformed_run_lease_is_recovered_instead_of_stuck_forever() {
        let pool = test_pool().await;
        let now = stamp(Utc::now());
        sqlx::query("INSERT INTO scheduled_task_runs(id,task_id,task_name,status,scheduled_at,lease_owner,lease_until,created_at,updated_at) VALUES('r-malformed','t1','task','running',?,'dead-worker','not-a-timestamp',?,?)")
            .bind(&now).bind(&now).bind(&now).execute(&pool).await.unwrap();

        recover_expired(&pool).await.unwrap();

        let status: String =
            sqlx::query_scalar("SELECT status FROM scheduled_task_runs WHERE id='r-malformed'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            status, "queued",
            "invalid lease timestamps must be treated as expired"
        );
    }
}
