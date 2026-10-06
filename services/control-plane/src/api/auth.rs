use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde_json::json;
use crate::AppState;

pub async fn get_current_user(State(state): State<AppState>) -> impl IntoResponse {
    // Look up or return default bootstrap user in v1
    let user_res = sqlx::query_as::<_, crate::models::User>(
        "SELECT id, workspace_id, email, display_name, role, created_at FROM users LIMIT 1"
    )
    .fetch_optional(&state.db)
    .await;

    match user_res {
        Ok(Some(user)) => (
            StatusCode::OK,
            Json(json!({
                "id": user.id,
                "workspace_id": user.workspace_id,
                "email": user.email,
                "display_name": user.display_name,
                "role": user.role,
                "permissions": ["suite:read", "suite:write", "run:trigger", "run:read", "admin:all"]
            })),
        ),
        _ => (
            StatusCode::OK,
            Json(json!({
                "id": "00000000-0000-0000-0000-000000000001",
                "workspace_id": "00000000-0000-0000-0000-000000000001",
                "email": "admin@testit.local",
                "display_name": "TestIT Admin",
                "role": "ADMIN",
                "permissions": ["suite:read", "suite:write", "run:trigger", "run:read", "admin:all"]
            })),
        ),
    }
}
