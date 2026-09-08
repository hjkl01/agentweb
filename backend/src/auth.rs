use crate::state::AppState;
use axum::{body::Body, extract::State, http::{header, Request, StatusCode}, middleware::Next, response::{IntoResponse, Response}, Json};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::Row;

async fn verify(state: &AppState, encoded: &str) -> bool {
    let Ok(decoded) = STANDARD.decode(encoded) else { return false; };
    let Ok(credentials) = String::from_utf8(decoded) else { return false; };
    let Some((username, password)) = credentials.split_once(':') else { return false; };
    let Some(stored_hash) = sqlx::query("SELECT password_hash FROM users WHERE username=?").bind(username).fetch_optional(&state.db).await.ok().flatten().map(|row| row.get::<String, _>(0)) else { return false; };
    format!("{:x}", Sha256::digest(password.as_bytes())) == stored_hash
}

fn unauthorized() -> Response { (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, r#"Basic realm="Agent Web""#)], "Authentication required").into_response() }
fn credentials(request: &Request<Body>) -> Option<String> {
    if let Some(value) = request.headers().get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Basic ")) { return Some(value.to_owned()); }
    let cookie = request.headers().get(header::COOKIE).and_then(|v| v.to_str().ok())?;
    cookie.split(';').map(str::trim).find_map(|item| item.strip_prefix("agentweb_auth=")).map(str::to_owned)
}

pub async fn basic_auth(State(state): State<AppState>, request: Request<Body>, next: Next) -> Response {
    let Some(encoded) = credentials(&request) else { return unauthorized(); };
    if !verify(&state, &encoded).await { return unauthorized(); }
    next.run(request).await
}

#[derive(Deserialize)]
pub struct LoginRequest { pub username: String, pub password: String }

pub async fn login(State(state): State<AppState>, Json(value): Json<LoginRequest>) -> Response {
    let raw = format!("{}:{}", value.username, value.password);
    let encoded = STANDARD.encode(raw);
    if !verify(&state, &encoded).await { return unauthorized(); }
    Response::builder()
        .status(StatusCode::OK)
        .header(header::SET_COOKIE, format!("agentweb_auth={encoded}; HttpOnly; SameSite=Strict; Path=/; Max-Age=2592000"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"status":"authenticated"}"#))
        .unwrap()
}
