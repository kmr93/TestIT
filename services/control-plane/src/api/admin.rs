use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::{
    api::{auth::hash_password, auth::AuthenticatedUser},
    AppState,
};

#[derive(Deserialize)]
pub struct CreateUserRequest {
    email: String,
    display_name: String,
    role: String,
    password: String,
}

#[derive(Deserialize)]
pub struct UpdateUserRequest {
    display_name: Option<String>,
    role: Option<String>,
    active: Option<bool>,
    password: Option<String>,
}

pub async fn list_users(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let users = sqlx::query_as::<_, (String, String, String, String, String, String, Option<String>)>(
        "SELECT users.id, users.email, users.display_name, users.role, users.workspace_id, users.created_at, disabled_users.disabled_at
         FROM users LEFT JOIN disabled_users ON disabled_users.user_id = users.id
         WHERE users.workspace_id = ? ORDER BY users.email",
    )
    .bind(&actor.workspace_id)
    .fetch_all(&state.db)
    .await;

    match users {
        Ok(users) => (
            StatusCode::OK,
            Json(json!(users
                .into_iter()
                .map(
                    |(id, email, display_name, role, workspace_id, created_at, disabled_at)| json!({
                        "id": id,
                        "email": email,
                        "display_name": display_name,
                        "role": role,
                        "workspace_id": workspace_id,
                        "created_at": created_at,
                        "active": disabled_at.is_none()
                    })
                )
                .collect::<Vec<_>>())),
        ),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Could not load workspace users" })),
        ),
    }
}

pub async fn create_user(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthenticatedUser>,
    Json(payload): Json<CreateUserRequest>,
) -> impl IntoResponse {
    let email = payload.email.trim().to_ascii_lowercase();
    let display_name = payload.display_name.trim();
    if email.is_empty() || email.len() > 320 || !email.contains('@') {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Enter a valid email address" })),
        );
    }
    if display_name.is_empty() || display_name.chars().count() > 128 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Display name must be between 1 and 128 characters" })),
        );
    }
    if !valid_role(&payload.role) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Role must be Admin, Author, Runner, or Viewer" })),
        );
    }
    if payload.password.chars().count() < 12 || payload.password.len() > 1024 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Password must be at least 12 characters" })),
        );
    }
    let password = payload.password;
    let password_hash = match tokio::task::spawn_blocking(move || hash_password(&password)).await {
        Ok(Ok(hash)) => hash,
        _ => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Could not secure the new account password" })),
            )
        }
    };

    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let mut tx = match state.db.begin().await {
        Ok(tx) => tx,
        Err(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": "User management is temporarily unavailable" })),
            )
        }
    };
    let inserted = sqlx::query(
        "INSERT INTO users (id, workspace_id, email, display_name, role, password_hash, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&actor.workspace_id)
    .bind(&email)
    .bind(display_name)
    .bind(&payload.role)
    .bind(password_hash)
    .bind(&now)
    .execute(&mut *tx)
    .await;
    if inserted.is_err() {
        let _ = tx.rollback().await;
        return (
            StatusCode::CONFLICT,
            Json(json!({ "error": "An account with this email already exists" })),
        );
    }
    if audit(
        &mut tx,
        &actor.id,
        "user.create",
        &id,
        json!({ "email": email, "role": payload.role }),
        &now,
    )
    .await
    .is_err()
    {
        let _ = tx.rollback().await;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Could not record the administrator action" })),
        );
    }
    if tx.commit().await.is_err() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Could not create the account" })),
        );
    }
    (
        StatusCode::CREATED,
        Json(
            json!({ "id": id, "email": email, "display_name": display_name, "role": payload.role, "active": true, "created_at": now }),
        ),
    )
}

pub async fn update_user(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
    Extension(actor): Extension<AuthenticatedUser>,
    Json(payload): Json<UpdateUserRequest>,
) -> impl IntoResponse {
    if user_id == actor.id {
        return (
            StatusCode::CONFLICT,
            Json(
                json!({ "error": "Use a different administrator account to change your own role or status" }),
            ),
        );
    }
    if payload
        .role
        .as_deref()
        .is_some_and(|role| !valid_role(role))
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Role must be Admin, Author, Runner, or Viewer" })),
        );
    }
    if payload
        .display_name
        .as_deref()
        .is_some_and(|name| name.trim().is_empty() || name.chars().count() > 128)
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Display name must be between 1 and 128 characters" })),
        );
    }
    if payload
        .password
        .as_deref()
        .is_some_and(|password| password.chars().count() < 12 || password.len() > 1024)
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Password must be at least 12 characters" })),
        );
    }
    if payload.display_name.is_none()
        && payload.role.is_none()
        && payload.active.is_none()
        && payload.password.is_none()
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Provide at least one account change" })),
        );
    }

    let existing = sqlx::query_as::<_, (String, String, Option<String>)>(
        "SELECT users.role, users.display_name, disabled_users.disabled_at FROM users
         LEFT JOIN disabled_users ON disabled_users.user_id = users.id
         WHERE users.id = ? AND users.workspace_id = ?",
    )
    .bind(&user_id)
    .bind(&actor.workspace_id)
    .fetch_optional(&state.db)
    .await
    .unwrap_or(None);
    let Some((old_role, old_name, old_disabled_at)) = existing else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "User not found" })),
        );
    };
    let active = payload.active.unwrap_or(old_disabled_at.is_none());
    let role = payload.role.clone().unwrap_or(old_role.clone());
    if old_role == "ADMIN" && old_disabled_at.is_none() && (!active || role != "ADMIN") {
        let active_admins: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM users WHERE workspace_id = ? AND role = 'ADMIN'
             AND NOT EXISTS (SELECT 1 FROM disabled_users WHERE disabled_users.user_id = users.id)",
        )
        .bind(&actor.workspace_id)
        .fetch_one(&state.db)
        .await
        .unwrap_or(0);
        if active_admins <= 1 {
            return (
                StatusCode::CONFLICT,
                Json(
                    json!({ "error": "The workspace must keep at least one active administrator" }),
                ),
            );
        }
    }

    let password_hash = if let Some(password) = payload.password {
        match tokio::task::spawn_blocking(move || hash_password(&password)).await {
            Ok(Ok(hash)) => Some(hash),
            _ => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": "Could not secure the account password" })),
                )
            }
        }
    } else {
        None
    };
    let password_reset = password_hash.is_some();
    let now = Utc::now().to_rfc3339();
    let next_name = payload.display_name.as_deref().unwrap_or(&old_name).trim();
    let mut tx = match state.db.begin().await {
        Ok(tx) => tx,
        Err(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": "User management is temporarily unavailable" })),
            )
        }
    };
    let update = sqlx::query(
        "UPDATE users SET display_name = ?, role = ?, password_hash = COALESCE(?, password_hash) WHERE id = ? AND workspace_id = ?",
    )
    .bind(next_name)
    .bind(&role)
    .bind(password_hash)
    .bind(&user_id)
    .bind(&actor.workspace_id)
    .execute(&mut *tx)
    .await;
    if update.is_err() {
        let _ = tx.rollback().await;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Could not update the account" })),
        );
    }
    if active {
        let _ = sqlx::query("DELETE FROM disabled_users WHERE user_id = ?")
            .bind(&user_id)
            .execute(&mut *tx)
            .await;
    } else {
        let _ = sqlx::query(
            "INSERT OR IGNORE INTO disabled_users (user_id, disabled_at) VALUES (?, ?)",
        )
        .bind(&user_id)
        .bind(&now)
        .execute(&mut *tx)
        .await;
        let _ = sqlx::query("DELETE FROM sessions WHERE user_id = ?")
            .bind(&user_id)
            .execute(&mut *tx)
            .await;
    }
    let changes = json!({
        "before": { "display_name": old_name, "role": old_role, "active": old_disabled_at.is_none() },
        "after": { "display_name": next_name, "role": role, "active": active, "password_reset": password_reset }
    });
    if audit(&mut tx, &actor.id, "user.update", &user_id, changes, &now)
        .await
        .is_err()
    {
        let _ = tx.rollback().await;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Could not record the administrator action" })),
        );
    }
    if tx.commit().await.is_err() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Could not save the account changes" })),
        );
    }
    (
        StatusCode::OK,
        Json(json!({ "id": user_id, "display_name": next_name, "role": role, "active": active })),
    )
}

fn valid_role(role: &str) -> bool {
    matches!(role, "ADMIN" | "AUTHOR" | "RUNNER" | "VIEWER")
}

async fn audit(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    actor_id: &str,
    action: &str,
    target_id: &str,
    changes: serde_json::Value,
    at: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO audit_events (id, actor_id, action, target_type, target_id, changes_json, created_at) VALUES (?, ?, ?, 'user', ?, ?, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(actor_id)
    .bind(action)
    .bind(target_id)
    .bind(changes.to_string())
    .bind(at)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
