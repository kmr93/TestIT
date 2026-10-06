use crate::AppState;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde_json::json;

pub async fn liveness_check() -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "service": "testit-control-plane",
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}

pub async fn metrics(State(state): State<AppState>) -> impl IntoResponse {
    let statuses = sqlx::query_as::<_, (String, i64)>(
        "SELECT status, COUNT(*) FROM suite_runs GROUP BY status",
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();
    let mut exposition = String::from(
        "# HELP testit_suite_runs Number of suite runs by status.\n# TYPE testit_suite_runs gauge\n",
    );
    for status in [
        "QUEUED",
        "RUNNING",
        "PASSED",
        "FAILED",
        "ERROR",
        "CANCELED",
        "INTERRUPTED",
    ] {
        let count = statuses
            .iter()
            .find(|(value, _)| value == status)
            .map_or(0, |(_, count)| *count);
        exposition.push_str(&format!(
            "testit_suite_runs{{status=\"{}\"}} {}\n",
            status.to_ascii_lowercase(),
            count
        ));
    }
    let pending_outbox = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM nats_event_outbox WHERE status = 'PENDING'",
    )
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);
    let queued = statuses
        .iter()
        .find(|(status, _)| status == "QUEUED")
        .map_or(0, |(_, count)| *count);
    let running = statuses
        .iter()
        .find(|(status, _)| status == "RUNNING")
        .map_or(0, |(_, count)| *count);
    exposition.push_str(&format!(
        "# HELP testit_run_queue_length Suite runs waiting for execution.\n# TYPE testit_run_queue_length gauge\ntestit_run_queue_length {}\n# HELP testit_active_suite_runs Suite runs currently executing.\n# TYPE testit_active_suite_runs gauge\ntestit_active_suite_runs {}\n# HELP testit_nats_outbox_pending Durable NATS events awaiting publication.\n# TYPE testit_nats_outbox_pending gauge\ntestit_nats_outbox_pending {}\n# HELP testit_nats_connected Whether the configured NATS connection is available.\n# TYPE testit_nats_connected gauge\ntestit_nats_connected {}\n",
        queued,
        running,
        pending_outbox,
        u8::from(state.nats.is_some())
    ));
    (
        StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        exposition,
    )
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
    let worker_status = if worker_engine_available(&state).await {
        "healthy"
    } else {
        "unavailable"
    };

    if db_status == "healthy" && worker_status == "healthy" {
        (
            StatusCode::OK,
            Json(json!({
                "status": "ready",
                "database": db_status,
                "worker_engine": worker_status,
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
                "worker_engine": worker_status,
                "nats_bus": nats_status,
                "timestamp": chrono::Utc::now().to_rfc3339()
            })),
        )
    }
}

pub async fn worker_engine_available(state: &AppState) -> bool {
    let Some(url) = state.config.worker_manager_url.as_deref() else {
        return false;
    };
    let Ok(client) = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
    else {
        return false;
    };
    client
        .get(format!("{}/health/live", url))
        .send()
        .await
        .is_ok_and(|response| response.status().is_success())
}
