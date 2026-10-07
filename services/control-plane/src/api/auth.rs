use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    body::Body,
    extract::{ConnectInfo, State},
    http::{header, HeaderMap, HeaderValue, Method, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Extension, Json,
};
use chrono::{Duration, Utc};
use rand::{rngs::OsRng, RngCore};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::net::SocketAddr;
use uuid::Uuid;

use crate::AppState;

const SESSION_COOKIE: &str = "testit_session";
const CSRF_COOKIE: &str = "testit_csrf";
const SESSION_HOURS: i64 = 8;
const LOGIN_LIMIT: i64 = 20;

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct AuthenticatedUser {
    pub id: String,
    pub workspace_id: String,
    pub email: String,
    pub display_name: String,
    pub role: String,
    pub token_hash: String,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    email: String,
    password: String,
}

pub fn hash_password(password: &str) -> Result<String, anyhow::Error> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|error| anyhow::anyhow!("Password hashing failed: {error}"))?
        .to_string())
}

pub async fn login(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(payload): Json<LoginRequest>,
) -> Response {
    let email = payload.email.trim().to_ascii_lowercase();
    if email.is_empty() || email.len() > 320 || payload.password.len() > 1024 {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"Invalid email or password"})),
        )
            .into_response();
    }

    let rate_key = format!("{}:{}", peer.ip(), email);
    let now = Utc::now().to_rfc3339();
    let rate_update = sqlx::query(
        "INSERT INTO login_attempts (identity_key, window_started_at, attempt_count)
         VALUES (?, ?, 1)
         ON CONFLICT(identity_key) DO UPDATE SET
           attempt_count = CASE WHEN datetime(login_attempts.window_started_at) < datetime('now', '-15 minutes') THEN 1 ELSE login_attempts.attempt_count + 1 END,
           window_started_at = CASE WHEN datetime(login_attempts.window_started_at) < datetime('now', '-15 minutes') THEN excluded.window_started_at ELSE login_attempts.window_started_at END",
    )
    .bind(&rate_key)
    .bind(&now)
    .execute(&state.db)
    .await;
    if rate_update.is_err() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"Login is temporarily unavailable"})),
        )
            .into_response();
    }
    let attempt_count: i64 =
        sqlx::query_scalar("SELECT attempt_count FROM login_attempts WHERE identity_key = ?")
            .bind(&rate_key)
            .fetch_one(&state.db)
            .await
            .unwrap_or(LOGIN_LIMIT + 1);
    if attempt_count > LOGIN_LIMIT {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [(header::RETRY_AFTER, "900")],
            Json(json!({"error":"Too many login attempts. Try again in 15 minutes."})),
        )
            .into_response();
    }

    let user: Option<(String, String, String, String, String)> = match sqlx::query_as(
        "SELECT id, workspace_id, email, display_name, password_hash FROM users
         WHERE email = ? AND NOT EXISTS (SELECT 1 FROM disabled_users WHERE disabled_users.user_id = users.id)",
    )
    .bind(&email)
    .fetch_optional(&state.db)
    .await
    {
        Ok(user) => user,
        Err(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error":"Login is temporarily unavailable"})),
            )
                .into_response();
        }
    };

    let candidate_hash = user.as_ref().map(|(_, _, _, _, hash)| hash.clone());
    let candidate_password = payload.password.clone();
    let valid_password = tokio::task::spawn_blocking(move || {
        if let Some(stored_hash) = candidate_hash {
            if let Ok(hash) = PasswordHash::new(&stored_hash) {
                return Argon2::default()
                    .verify_password(candidate_password.as_bytes(), &hash)
                    .is_ok();
            }
        }
        // Consume comparable Argon2 work for absent accounts and invalid legacy hashes.
        let salt = SaltString::generate(&mut OsRng);
        let _ = Argon2::default().hash_password(candidate_password.as_bytes(), &salt);
        false
    })
    .await
    .unwrap_or(false);

    let Some((user_id, _workspace_id, user_email, display_name, _)) =
        user.filter(|_| valid_password)
    else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"Invalid email or password"})),
        )
            .into_response();
    };

    let _ = sqlx::query("DELETE FROM login_attempts WHERE identity_key = ?")
        .bind(&rate_key)
        .execute(&state.db)
        .await;

    let mut token_bytes = [0u8; 32];
    let mut csrf_bytes = [0u8; 32];
    OsRng.fill_bytes(&mut token_bytes);
    OsRng.fill_bytes(&mut csrf_bytes);
    let token = hex::encode(token_bytes);
    let csrf = hex::encode(csrf_bytes);
    let token_hash = hash_token(&token);
    let expires = Utc::now() + Duration::hours(SESSION_HOURS);
    let expires_at = expires.to_rfc3339();

    if sqlx::query("INSERT INTO sessions (id, user_id, token_hash, expires_at, created_at) VALUES (?, ?, ?, ?, ?)")
        .bind(Uuid::new_v4().to_string())
        .bind(&user_id)
        .bind(&token_hash)
        .bind(&expires_at)
        .bind(Utc::now().to_rfc3339())
        .execute(&state.db)
        .await
        .is_err()
    {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"Could not create a session"}))).into_response();
    }

    let user_row = sqlx::query_as::<_, (String,)>("SELECT role FROM users WHERE id = ?")
        .bind(&user_id)
        .fetch_one(&state.db)
        .await;
    let role = user_row
        .map(|row| row.0)
        .unwrap_or_else(|_| "VIEWER".to_string());
    let permissions = permissions_for(&role);
    let mut headers = HeaderMap::new();
    if let Some(cookie) = cookie_header(
        SESSION_COOKIE,
        &token,
        true,
        SESSION_HOURS * 3600,
        state.config.cookie_secure,
    ) {
        headers.append(header::SET_COOKIE, cookie);
    }
    if let Some(cookie) = cookie_header(
        CSRF_COOKIE,
        &csrf,
        false,
        SESSION_HOURS * 3600,
        state.config.cookie_secure,
    ) {
        headers.append(header::SET_COOKIE, cookie);
    }

    let _ = sqlx::query(
        "INSERT INTO audit_events (id, actor_id, action, target_type, target_id, changes_json, created_at)
         VALUES (?, ?, 'auth.login', 'user', ?, '{}', ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(&user_id)
    .bind(&user_id)
    .bind(Utc::now().to_rfc3339())
    .execute(&state.db)
    .await;

    (
        StatusCode::OK,
        headers,
        Json(json!({
            "id": user_id,
            "email": user_email,
            "display_name": display_name,
            "role": role,
            "permissions": permissions,
            "expires_at": expires_at
        })),
    )
        .into_response()
}

pub async fn logout(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    let _ = sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
        .bind(&user.token_hash)
        .execute(&state.db)
        .await;
    let _ = sqlx::query(
        "INSERT INTO audit_events (id, actor_id, action, target_type, target_id, changes_json, created_at)
         VALUES (?, ?, 'auth.logout', 'user', ?, '{}', ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(&user.id)
    .bind(&user.id)
    .bind(Utc::now().to_rfc3339())
    .execute(&state.db)
    .await;

    let mut headers = HeaderMap::new();
    for name in [SESSION_COOKIE, CSRF_COOKIE] {
        if let Some(cookie) = cookie_header(
            name,
            "",
            name == SESSION_COOKIE,
            0,
            state.config.cookie_secure,
        ) {
            headers.append(header::SET_COOKIE, cookie);
        }
    }
    (
        StatusCode::OK,
        headers,
        Json(json!({"status":"signed_out"})),
    )
        .into_response()
}

pub async fn get_current_user(Extension(user): Extension<AuthenticatedUser>) -> impl IntoResponse {
    Json(json!({
        "id": user.id,
        "workspace_id": user.workspace_id,
        "email": user.email,
        "display_name": user.display_name,
        "role": user.role,
        "permissions": permissions_for(&user.role)
    }))
}

pub async fn require_session(
    State(state): State<AppState>,
    mut request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let token = request
        .headers()
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| cookie_value(value, SESSION_COOKIE))
        .ok_or(StatusCode::UNAUTHORIZED)?;
    if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let token_hash = hash_token(token);
    let user = sqlx::query_as::<_, AuthenticatedUser>(
        "SELECT users.id, users.workspace_id, users.email, users.display_name, users.role, sessions.token_hash
         FROM sessions JOIN users ON users.id = sessions.user_id
         WHERE sessions.token_hash = ? AND sessions.expires_at > ?
           AND NOT EXISTS (SELECT 1 FROM disabled_users WHERE disabled_users.user_id = users.id)",
    )
    .bind(&token_hash)
    .bind(Utc::now().to_rfc3339())
    .fetch_optional(&state.db)
    .await
    .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
    .ok_or(StatusCode::UNAUTHORIZED)?;

    let method = request.method().clone();
    if !matches!(method, Method::GET | Method::HEAD | Method::OPTIONS) {
        let csrf_cookie = request
            .headers()
            .get(header::COOKIE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| cookie_value(value, CSRF_COOKIE));
        let csrf_header = request
            .headers()
            .get("x-csrf-token")
            .and_then(|value| value.to_str().ok());
        if csrf_cookie.is_none() || csrf_cookie != csrf_header {
            return Err(StatusCode::FORBIDDEN);
        }
    }

    let path = request.uri().path();
    if !role_can_access(&user.role, &method, path) {
        return Err(StatusCode::FORBIDDEN);
    }
    request.extensions_mut().insert(user);
    Ok(next.run(request).await)
}

fn role_can_access(role: &str, method: &Method, path: &str) -> bool {
    if role == "ADMIN" {
        return true;
    }
    match (role, method.as_str(), path) {
        (_, "GET", "/api/v1/me") => true,
        ("AUTHOR", "POST", "/api/v1/assets") => true,
        ("AUTHOR", "PATCH", _)
            if path.starts_with("/api/v1/assets/") && path.ends_with("/draft") =>
        {
            true
        }
        ("AUTHOR", "POST", _)
            if path.starts_with("/api/v1/assets/") && path.ends_with("/publish") =>
        {
            true
        }
        ("AUTHOR", "GET", _) if path.starts_with("/api/v1/assets/") && path.ends_with("/draft") => {
            true
        }
        ("AUTHOR", "GET", "/api/v1/environments") | ("RUNNER", "GET", "/api/v1/environments") => {
            true
        }
        ("AUTHOR", "GET", "/api/v1/connections") | ("RUNNER", "GET", "/api/v1/connections") => true,
        ("AUTHOR", "GET", "/api/v1/portability/export") => true,
        ("AUTHOR", "POST", "/api/v1/portability/preview")
        | ("AUTHOR", "POST", "/api/v1/portability/import") => true,
        ("AUTHOR", "POST", "/api/v1/specifications/openapi/validate")
        | ("AUTHOR", "POST", "/api/v1/specifications/openapi/import")
        | ("AUTHOR", "POST", "/api/v1/variables/preview") => true,
        ("RUNNER", "POST", "/api/v1/runs") | ("RUNNER", "POST", "/api/v1/variables/preview") => {
            true
        }
        (_, "GET", _) if path.starts_with("/api/v1/assets/") && path.ends_with("/draft") => false,
        (_, "GET", "/api/v1/assets") => true,
        (_, "GET", _) if path.starts_with("/api/v1/assets/") && path.contains("/revisions") => true,
        (_, "GET", _) if path.starts_with("/api/v1/runs") => true,
        ("RUNNER", "POST", _)
            if path.starts_with("/api/v1/runs/")
                && (path.ends_with("/cancel") || path.ends_with("/rerun-failed")) =>
        {
            true
        }
        ("VIEWER", "POST", "/api/v1/variables/preview") => true,
        _ => false,
    }
}

fn permissions_for(role: &str) -> Vec<&'static str> {
    match role {
        "ADMIN" => vec![
            "suite:read",
            "suite:write",
            "run:trigger",
            "run:read",
            "admin:all",
        ],
        "AUTHOR" => vec!["suite:read", "suite:write", "run:read", "variable:preview"],
        "RUNNER" => vec!["suite:read", "run:trigger", "run:read", "variable:preview"],
        _ => vec!["suite:read", "run:read", "variable:preview"],
    }
}

fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn cookie_value<'a>(cookies: &'a str, name: &str) -> Option<&'a str> {
    cookies.split(';').find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        (key == name).then_some(value)
    })
}

fn cookie_header(
    name: &str,
    value: &str,
    http_only: bool,
    max_age: i64,
    secure: bool,
) -> Option<HeaderValue> {
    let mut cookie = format!("{name}={value}; Path=/; SameSite=Strict; Max-Age={max_age}");
    if http_only {
        cookie.push_str("; HttpOnly");
    }
    if secure {
        cookie.push_str("; Secure");
    }
    HeaderValue::from_str(&cookie).ok()
}

#[cfg(test)]
mod tests {
    use super::{hash_password, role_can_access};
    use argon2::PasswordVerifier;
    use axum::http::Method;

    #[test]
    fn password_hash_is_salted_and_verifiable() {
        let first = hash_password("correct horse battery staple").unwrap();
        let second = hash_password("correct horse battery staple").unwrap();
        assert_ne!(first, second);
        let parsed = argon2::password_hash::PasswordHash::new(&first).unwrap();
        assert!(argon2::Argon2::default()
            .verify_password(b"correct horse battery staple", &parsed)
            .is_ok());
        assert!(argon2::Argon2::default()
            .verify_password(b"incorrect", &parsed)
            .is_err());
    }

    #[test]
    fn roles_restrict_mutating_routes() {
        assert!(!role_can_access("VIEWER", &Method::POST, "/api/v1/runs"));
        assert!(role_can_access("RUNNER", &Method::POST, "/api/v1/runs"));
        assert!(!role_can_access(
            "RUNNER",
            &Method::PATCH,
            "/api/v1/assets/1/draft"
        ));
        assert!(role_can_access(
            "AUTHOR",
            &Method::PATCH,
            "/api/v1/assets/1/draft"
        ));
    }
}
