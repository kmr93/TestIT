use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde_json::json;
use crate::AppState;

pub async fn liveness_check() -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "service": "testit-control-plane",
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}

pub async fn readiness_check(State(state): State<AppState>) -> impl IntoResponse {
    // Check SQLite readiness
    let db_status = match sqlx::query("SELECT 1").execute(&state.db).await {
        Ok(_) => "healthy",
        Err(e) => {
            tracing::error!("Database readiness check failed: {}", e);
            "unhealthy"
        }
    };

    let nats_status = if state.nats.is_some() {
        "connected"
    } else {
        "degraded_offline"
    };

    if db_status == "healthy" {
        (
            StatusCode::OK,
            Json(json!({
                "status": "ready",
                "database": db_status,
                "nats_bus": nats_status,
                "timestamp": chrono::Utc::now().to_rfc3339()
            })),
        )
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "status": "unhealthy",
                "database": db_status,
                "nats_bus": nats_status,
                "timestamp": chrono::Utc::now().to_rfc3339()
            })),
        )
    }
}
