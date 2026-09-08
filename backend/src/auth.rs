use crate::state::AppState;
use axum::{body::Body, extract::State, http::{header, Request, StatusCode}, middleware::Next, response::{IntoResponse, Response}, Json};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::{Duration, Utc};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::Row;
use uuid::Uuid;

async fn verify_password(state: &AppState, username: &str, password: &str) -> bool {
    let Some(stored_hash) = sqlx::query("SELECT password_hash FROM users WHERE username=?")
        .bind(username).fetch_optional(&state.db).await.ok().flatten()
        .map(|row| row.get::<String, _>(0)) else { return false; };
    format!("{:x}", Sha256::digest(password.as_bytes())) == stored_hash
}

fn unauthorized() -> Response { (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, r#"Basic realm=\"Agent Web\""#)], "Authentication required").into_response() }

fn cookie_token(request: &Request<Body>) -> Option<String> {
    let cookie = request.headers().get(header::COOKIE).and_then(|v| v.to_str().ok())?;
    cookie.split(';').map(str::trim).find_map(|item| item.strip_prefix("agentweb_session=")).map(str::to_owned)
}

async fn valid_session(state: &AppState, token: &str) -> bool {
    let hash = format!("{:x}", Sha256::digest(token.as_bytes()));
    let now = Utc::now().to_rfc3339();
    sqlx::query("SELECT id FROM auth_sessions WHERE token_hash=? AND expires_at>? LIMIT 1")
        .bind(hash).bind(now).fetch_optional(&state.db).await.ok().flatten().is_some()
}

fn basic_credentials(request: &Request<Body>) -> Option<(String, String)> {
    let value = request.headers().get(header::AUTHORIZATION)?.to_str().ok()?.strip_prefix("Basic ")?;
    let decoded = STANDARD.decode(value).ok()?;
    let credentials = String::from_utf8(decoded).ok()?;
    credentials.split_once(':').map(|(u, p)| (u.to_owned(), p.to_owned()))
}

pub async fn basic_auth(State(state): State<AppState>, request: Request<Body>, next: Next) -> Response {
    if let Some(token) = cookie_token(&request) {
        if valid_session(&state, &token).await { return next.run(request).await; }
    }
    if let Some((username, password)) = basic_credentials(&request) {
        if verify_password(&state, &username, &password).await { return next.run(request).await; }
    }
    unauthorized()
}

pub async fn me(State(state): State<AppState>, request: Request<Body>) -> Response {
    if let Some(token) = cookie_token(&request) {
        if valid_session(&state, &token).await { return (StatusCode::OK, Json(serde_json::json!({ "authenticated": true }))).into_response(); }
    }
    unauthorized()
}

#[derive(Deserialize)]
pub struct LoginRequest { pub username: String, pub password: String }

pub async fn login(State(state): State<AppState>, Json(value): Json<LoginRequest>) -> Response {
    if !verify_password(&state, &value.username, &value.password).await { return unauthorized(); }
    let token = Uuid::new_v4().to_string();
    let hash = format!("{:x}", Sha256::digest(token.as_bytes()));
    let now = Utc::now();
    let expires = now + Duration::days(30);
    let user_id: Option<String> = sqlx::query("SELECT id FROM users WHERE username=?").bind(&value.username).fetch_optional(&state.db).await.ok().flatten().map(|row| row.get(0));
    let Some(user_id) = user_id else { return unauthorized(); };
    if sqlx::query("INSERT INTO auth_sessions(id,user_id,token_hash,expires_at,created_at) VALUES(?,?,?,?,?)")
        .bind(Uuid::new_v4().to_string()).bind(user_id).bind(hash).bind(expires.to_rfc3339()).bind(now.to_rfc3339()).execute(&state.db).await.is_err() { return (StatusCode::INTERNAL_SERVER_ERROR, "Unable to create session").into_response(); }
    Response::builder().status(StatusCode::OK)
        .header(header::SET_COOKIE, format!("agentweb_session={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=2592000"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"status":"authenticated"}"#)).unwrap()
}

pub async fn logout(State(state): State<AppState>, request: Request<Body>) -> Response {
    if let Some(token) = cookie_token(&request) {
        let hash = format!("{:x}", Sha256::digest(token.as_bytes()));
        let _ = sqlx::query("DELETE FROM auth_sessions WHERE token_hash=?").bind(hash).execute(&state.db).await;
    }
    Response::builder().status(StatusCode::OK)
        .header(header::SET_COOKIE, "agentweb_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"status":"logged_out"}"#)).unwrap()
}
