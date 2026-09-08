use crate::state::AppState;
use axum::{body::Body, http::{header, Request, StatusCode}, middleware::Next, response::Response};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use sha2::{Digest, Sha256};
use sqlx::Row;

fn unauthorized() -> Response {
    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header(header::WWW_AUTHENTICATE, r#"Basic realm="Agent Web""#)
        .body(Body::from("Authentication required"))
        .unwrap()
}

pub async fn basic_auth(State(state): axum::extract::State<AppState>, request: Request<Body>, next: Next) -> Response {
    let Some(value) = request.headers().get(header::AUTHORIZATION).and_then(|value| value.to_str().ok()) else { return unauthorized(); };
    let Some(encoded) = value.strip_prefix("Basic ") else { return unauthorized(); };
    let Ok(decoded) = STANDARD.decode(encoded) else { return unauthorized(); };
    let Ok(credentials) = String::from_utf8(decoded) else { return unauthorized(); };
    let Some((username, password)) = credentials.split_once(':') else { return unauthorized(); };
    let Some(stored_hash) = sqlx::query("SELECT password_hash FROM users WHERE username=?")
        .bind(username).fetch_optional(&state.db).await.ok().flatten().map(|row| row.get::<String, _>(0)) else { return unauthorized(); };
    let supplied_hash = format!("{:x}", Sha256::digest(password.as_bytes()));
    if supplied_hash != stored_hash { return unauthorized(); }
    next.run(request).await
}
