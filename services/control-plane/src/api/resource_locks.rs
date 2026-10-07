use axum::{extract::State, http::StatusCode, response::IntoResponse, Extension, Json};
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::{api::auth::AuthenticatedUser, AppState};

#[derive(Deserialize)]
pub struct ReleaseLockRequest {
    pub resource_key: String,
    pub reason: String,
}

pub async fn list_resource_locks(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let result = sqlx::query_as::<_, (String, String, String, String, String, String)>(
        "SELECT locks.resource_key, locks.run_id, locks.owner, locks.status,
                locks.lease_expires_at, runs.status
         FROM resource_locks locks
         JOIN suite_runs runs ON runs.id = locks.run_id
         WHERE runs.workspace_id = ?
         ORDER BY locks.status, locks.resource_key",
    )
    .bind(&user.workspace_id)
    .fetch_all(&state.db)
    .await;
    match result {
        Ok(rows) => Json(json!(rows
            .into_iter()
            .map(
                |(resource_key, run_id, owner_id, status, lease_expires_at, run_status)| {
                    json!({
                        "resource_key": resource_key,
                        "run_id": run_id,
                        "owner_id": owner_id,
                        "status": status,
                        "lease_expires_at": lease_expires_at,
                        "run_status": run_status
                    })
                }
            )
            .collect::<Vec<_>>()))
        .into_response(),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "Could not read resource locks" })),
        )
            .into_response(),
    }
}

pub async fn release_resource_lock(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(payload): Json<ReleaseLockRequest>,
) -> impl IntoResponse {
    let reason = payload.reason.trim();
    if payload.resource_key.trim().is_empty()
        || payload.resource_key.len() > 180
        || reason.chars().count() < 5
        || reason.chars().count() > 500
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                json!({ "error": "Provide a resource lock and a 5–500 character release reason" }),
            ),
        )
            .into_response();
    }
    let mut tx = match state.db.begin().await {
        Ok(tx) => tx,
        Err(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": "Resource lock service is temporarily unavailable" })),
            )
                .into_response()
        }
    };
    let lock = sqlx::query_as::<_, (String, String, String, String, i64)>(
        "SELECT locks.run_id, locks.status, runs.status, locks.lease_expires_at,
                CASE WHEN julianday(locks.lease_expires_at) <= julianday('now') THEN 1 ELSE 0 END
         FROM resource_locks locks
         JOIN suite_runs runs ON runs.id = locks.run_id
         WHERE locks.resource_key = ? AND runs.workspace_id = ?",
    )
    .bind(&payload.resource_key)
    .bind(&user.workspace_id)
    .fetch_optional(&mut *tx)
    .await;
    let (run_id, lock_status, run_status, lease_expires_at, lease_expired) = match lock {
        Ok(Some(lock)) => lock,
        Ok(None) => {
            let _ = tx.rollback().await;
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "Resource lock was not found" })),
            )
                .into_response();
        }
        Err(_) => {
            let _ = tx.rollback().await;
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": "Could not read resource lock state" })),
            )
                .into_response();
        }
    };
    if matches!(run_status.as_str(), "RUNNING" | "QUEUED") {
        if lock_status == "HELD" && lease_expired == 1 {
            let now = Utc::now().to_rfc3339();
            if sqlx::query(
                "UPDATE resource_locks SET status = 'UNCERTAIN' WHERE resource_key = ? AND run_id = ? AND status = 'HELD'",
            )
            .bind(&payload.resource_key)
            .bind(&run_id)
            .execute(&mut *tx)
            .await
            .is_err()
                || record_lock_audit(
                    &mut tx,
                    &user.id,
                    &payload.resource_key,
                    &run_id,
                    "resource_lock.expired",
                    "Lease expired while the run was still active; lock requires review",
                    &now,
                )
                .await
                .is_err()
                || tx.commit().await.is_err()
            {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": "Could not mark the expired lock as uncertain" })),
                )
                    .into_response();
            }
            return (
                StatusCode::CONFLICT,
                Json(json!({ "error": "The lease expired while its run is active. The lock is now marked uncertain and needs review before release." })),
            )
                .into_response();
        }
        let _ = tx.rollback().await;
        return (
            StatusCode::CONFLICT,
            Json(json!({ "error": "The run is still queued or running. Do not release its lock until the run is terminal." })),
        )
            .into_response();
    }
    let now = Utc::now().to_rfc3339();
    if record_lock_audit(
        &mut tx,
        &user.id,
        &payload.resource_key,
        &run_id,
        "resource_lock.release",
        reason,
        &now,
    )
    .await
    .is_err()
        || sqlx::query("DELETE FROM resource_locks WHERE resource_key = ? AND run_id = ?")
            .bind(&payload.resource_key)
            .bind(&run_id)
            .execute(&mut *tx)
            .await
            .is_err()
        || tx.commit().await.is_err()
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Could not release and audit the resource lock" })),
        )
            .into_response();
    }
    Json(json!({
        "status": "released",
        "resource_key": payload.resource_key,
        "run_id": run_id,
        "previous_status": lock_status,
        "previous_lease_expires_at": lease_expires_at
    }))
    .into_response()
}

async fn record_lock_audit(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    actor_id: &str,
    resource_key: &str,
    run_id: &str,
    action: &str,
    reason: &str,
    now: &str,
) -> Result<(), sqlx::Error> {
    let changes = json!({ "resource_key": resource_key, "run_id": run_id, "reason": reason });
    sqlx::query(
        "INSERT INTO audit_events (id, actor_id, action, target_type, target_id, changes_json, created_at)
         VALUES (?, ?, ?, 'resource_lock', ?, ?, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(actor_id)
    .bind(action)
    .bind(resource_key)
    .bind(changes.to_string())
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
