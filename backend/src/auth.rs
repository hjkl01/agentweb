use crate::state::AppState;
use axum::{body::Body, extract::{ConnectInfo, State}, http::{header, HeaderMap, Request, StatusCode}, middleware::Next, response::{IntoResponse, Response}, Json};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use argon2::{password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString}, Argon2};
use argon2::password_hash::rand_core::OsRng;
use sqlx::Row;
use uuid::Uuid;
use std::net::{IpAddr, SocketAddr};

const MAX_FAILURES: i64 = 3;
const LOCK_SECONDS: i64 = 30 * 60;

fn hash(value: &str) -> String { format!("{:x}", Sha256::digest(value.as_bytes())) }
fn unauthorized() -> Response { (StatusCode::UNAUTHORIZED, "Authentication required").into_response() }
fn cookie_token_from_headers(headers: &HeaderMap) -> Option<String> { headers.get(header::COOKIE).and_then(|v| v.to_str().ok())?.split(';').map(str::trim).find_map(|x| x.strip_prefix("agentweb_session=")).map(str::to_owned) }
fn cookie_token(request: &Request<Body>) -> Option<String> { cookie_token_from_headers(request.headers()) }
fn trusted_proxy_ips() -> Vec<IpAddr> {
    std::env::var("AGENTWEB_TRUSTED_PROXIES")
        .unwrap_or_default()
        .split(',')
        .filter_map(|value| value.trim().parse().ok())
        .collect()
}

fn client_ip(headers: &HeaderMap, peer: SocketAddr, trusted: &[IpAddr]) -> String {
    // Forwarded headers are user-controlled unless the direct TCP peer is a trusted proxy.
    if !trusted.contains(&peer.ip()) {
        return peer.ip().to_string();
    }

    if let Some(chain) = headers.get("x-forwarded-for").and_then(|value| value.to_str().ok()) {
        let forwarded: Vec<IpAddr> = chain.split(',').filter_map(|value| value.trim().parse().ok()).collect();
        let mut current = peer.ip();
        for address in forwarded.iter().rev() {
            if !trusted.contains(&current) {
                break;
            }
            current = *address;
        }
        if !trusted.contains(&current) {
            return current.to_string();
        }
    }

    headers.get("x-real-ip")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<IpAddr>().ok())
        .map(|address| address.to_string())
        .unwrap_or_else(|| peer.ip().to_string())
}

fn secure_cookie_attribute() -> &'static str {
    if std::env::var("AGENTWEB_COOKIE_SECURE")
        .map(|value| value.eq_ignore_ascii_case("false") || value == "0")
        .unwrap_or(false)
    {
        ""
    } else {
        "; Secure"
    }
}

async fn locked(state: &AppState, ip: &str) -> bool {
    sqlx::query_scalar::<_, i64>("SELECT locked_until FROM login_failures WHERE client_ip=?")
        .bind(ip)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
        .map(|until| until > Utc::now().timestamp())
        .unwrap_or(false)
}

async fn record_failure(state: &AppState, ip: &str) {
    let now = Utc::now();
    let _ = sqlx::query(r#"
        INSERT INTO login_failures(client_ip, failures, locked_until, updated_at)
        VALUES(?, 1, 0, ?)
        ON CONFLICT(client_ip) DO UPDATE SET
            failures = login_failures.failures + 1,
            locked_until = CASE
                WHEN login_failures.failures + 1 >= ?
                THEN unixepoch('now') + ?
                ELSE login_failures.locked_until
            END,
            updated_at = excluded.updated_at
    "#)
    .bind(ip)
    .bind(now.to_rfc3339())
    .bind(MAX_FAILURES)
    .bind(LOCK_SECONDS)
    .execute(&state.db)
    .await;
}

async fn clear_failures(state: &AppState, ip: &str) {
    let _ = sqlx::query("DELETE FROM login_failures WHERE client_ip=?")
        .bind(ip)
        .execute(&state.db)
        .await;
}

fn password_hash(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default().hash_password(password.as_bytes(), &salt)
        .map_err(|error| anyhow::anyhow!("password hashing failed: {error}"))?
        .to_string())
}

async fn verify_password(state: &AppState, username: &str, password: &str) -> Option<String> {
    let row = sqlx::query("SELECT id, password_hash FROM users WHERE username=?")
        .bind(username).fetch_optional(&state.db).await.ok().flatten()?;
    let user_id: String = row.get(0);
    let stored: String = row.get(1);

    if stored.starts_with("$argon2id$") {
        let parsed = PasswordHash::new(&stored).ok()?;
        return Argon2::default().verify_password(password.as_bytes(), &parsed)
            .is_ok().then_some(user_id);
    }

    // Backward compatibility: existing installations stored a plain SHA-256 digest.
    if hash(password) != stored {
        return None;
    }

    // Upgrade only after the legacy password has been verified.
    if let Ok(upgraded) = password_hash(password) {
        let _ = sqlx::query("UPDATE users SET password_hash=?, updated_at=? WHERE id=? AND password_hash=?")
            .bind(upgraded).bind(Utc::now().to_rfc3339()).bind(&user_id).bind(&stored)
            .execute(&state.db).await;
    }
    Some(user_id)
}
async fn valid_session(state: &AppState, token: &str) -> Option<(String, String)> { let row = sqlx::query("SELECT s.id, u.username FROM auth_sessions s JOIN users u ON u.id=s.user_id WHERE s.token_hash=? AND s.expires_at>? LIMIT 1").bind(hash(token)).bind(Utc::now().to_rfc3339()).fetch_optional(&state.db).await.ok().flatten()?; Some((row.get(0), row.get(1))) }

pub async fn basic_auth(State(state): State<AppState>, request: Request<Body>, next: Next) -> Response { if let Some(token) = cookie_token(&request) { if valid_session(&state, &token).await.is_some() { return next.run(request).await; } } unauthorized() }
pub async fn me(State(state): State<AppState>, request: Request<Body>) -> Response { let Some(token) = cookie_token(&request) else { return unauthorized(); }; let Some((session_id, username)) = valid_session(&state,&token).await else { return unauthorized(); }; (StatusCode::OK, Json(serde_json::json!({"authenticated":true,"username":username,"session_id":session_id}))).into_response() }

#[derive(Deserialize)] pub struct LoginRequest { pub username: String, pub password: String }
#[derive(Deserialize)] pub struct ChangePasswordRequest { pub current_password: String, pub new_password: String }
#[derive(Serialize)] pub struct AuthSession { pub id: String, pub created_at: String, pub expires_at: String, pub current: bool }

pub async fn login(State(state): State<AppState>, ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap, Json(value): Json<LoginRequest>) -> Response { let trusted = trusted_proxy_ips(); let ip = client_ip(&headers, peer, &trusted); if locked(&state,&ip).await { return (StatusCode::TOO_MANY_REQUESTS, "Too many failed login attempts. Try again in 30 minutes.").into_response(); } let Some(user_id) = verify_password(&state,&value.username,&value.password).await else { record_failure(&state,&ip).await; return unauthorized(); }; clear_failures(&state,&ip).await; let token = Uuid::new_v4().to_string(); let now = Utc::now(); let expires = now + Duration::days(30); if sqlx::query("INSERT INTO auth_sessions(id,user_id,token_hash,expires_at,created_at) VALUES(?,?,?,?,?)").bind(Uuid::new_v4().to_string()).bind(user_id).bind(hash(&token)).bind(expires.to_rfc3339()).bind(now.to_rfc3339()).execute(&state.db).await.is_err() { return (StatusCode::INTERNAL_SERVER_ERROR,"Unable to create session").into_response(); } Response::builder().status(StatusCode::OK).header(header::SET_COOKIE,format!("agentweb_session={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=2592000{}", secure_cookie_attribute())).header(header::CONTENT_TYPE,"application/json").body(Body::from(r#"{"status":"authenticated"}"#)).unwrap() }

pub async fn logout(State(state): State<AppState>, request: Request<Body>) -> Response { if let Some(token)=cookie_token(&request) { let _=sqlx::query("DELETE FROM auth_sessions WHERE token_hash=?").bind(hash(&token)).execute(&state.db).await; } clear_cookie() }
fn clear_cookie() -> Response { Response::builder().status(StatusCode::OK).header(header::SET_COOKIE,format!("agentweb_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0{}", secure_cookie_attribute())).header(header::CONTENT_TYPE,"application/json").body(Body::from(r#"{"status":"logged_out"}"#)).unwrap() }
async fn current_user(state: &AppState, token: Option<String>) -> Option<(String,String)> { valid_session(state, &token?).await }

pub async fn change_password(State(state): State<AppState>, headers: HeaderMap, Json(value): Json<ChangePasswordRequest>) -> Response { let token = cookie_token_from_headers(&headers); let Some((session_id,username))=current_user(&state,token).await else { return unauthorized(); }; if value.new_password.len() < 8 { return (StatusCode::BAD_REQUEST,"Password must contain at least 8 characters").into_response(); } if verify_password(&state,&username,&value.current_password).await.is_none() { return (StatusCode::BAD_REQUEST,"Current password is incorrect").into_response(); } if sqlx::query("UPDATE users SET password_hash=?,updated_at=? WHERE username=?").bind(match password_hash(&value.new_password) { Ok(value) => value, Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR,"Unable to hash password").into_response() }).bind(Utc::now().to_rfc3339()).bind(&username).execute(&state.db).await.is_err() { return (StatusCode::INTERNAL_SERVER_ERROR,"Unable to update password").into_response(); } let _=sqlx::query("DELETE FROM auth_sessions WHERE id<>? AND user_id=(SELECT user_id FROM auth_sessions WHERE id=?)").bind(&session_id).bind(&session_id).execute(&state.db).await; (StatusCode::OK,Json(serde_json::json!({"status":"password_changed","username":username}))).into_response() }
pub async fn list_sessions(State(state): State<AppState>, request: Request<Body>) -> Response { let token = cookie_token(&request); let Some((current_id,_))=current_user(&state,token).await else { return unauthorized(); }; let rows=sqlx::query("SELECT id,created_at,expires_at FROM auth_sessions WHERE user_id=(SELECT user_id FROM auth_sessions WHERE id=?) AND expires_at>? ORDER BY created_at DESC").bind(&current_id).bind(Utc::now().to_rfc3339()).fetch_all(&state.db).await.unwrap_or_default(); let sessions=rows.into_iter().map(|r|AuthSession{id:r.get(0),created_at:r.get(1),expires_at:r.get(2),current:r.get::<String,_>(0)==current_id}).collect::<Vec<_>>(); (StatusCode::OK,Json(sessions)).into_response() }
pub async fn revoke_all_sessions(State(state): State<AppState>, request: Request<Body>) -> Response { let token = cookie_token(&request); let Some((current_id,_))=current_user(&state,token).await else { return unauthorized(); }; let _=sqlx::query("DELETE FROM auth_sessions WHERE id<>? AND user_id=(SELECT user_id FROM auth_sessions WHERE id=?)").bind(&current_id).bind(&current_id).execute(&state.db).await; (StatusCode::OK,Json(serde_json::json!({"status":"revoked"}))).into_response() }


#[cfg(test)]
mod password_hash_tests {
    use super::*;

    #[test]
    fn argon2_hashes_verify_and_do_not_store_plaintext() {
        let encoded = password_hash("correct horse battery staple").unwrap();
        assert!(encoded.starts_with("$argon2id$"));
        assert!(!encoded.contains("correct horse"));
        let parsed = PasswordHash::new(&encoded).unwrap();
        assert!(Argon2::default().verify_password(b"correct horse battery staple", &parsed).is_ok());
        assert!(Argon2::default().verify_password(b"wrong password", &parsed).is_err());
    }
}

#[cfg(test)]
mod client_ip_tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn ignores_forwarded_headers_from_untrusted_peers() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("198.51.100.7"));
        let peer: SocketAddr = "203.0.113.5:12345".parse().unwrap();
        let trusted: Vec<IpAddr> = vec!["127.0.0.1".parse().unwrap()];
        assert_eq!(client_ip(&headers, peer, &trusted), "203.0.113.5");
    }

    #[test]
    fn resolves_client_ip_through_trusted_proxy_chain() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("198.51.100.7, 10.0.0.2"));
        let peer: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let trusted: Vec<IpAddr> = vec!["127.0.0.1".parse().unwrap(), "10.0.0.2".parse().unwrap()];
        assert_eq!(client_ip(&headers, peer, &trusted), "198.51.100.7");
    }
}
