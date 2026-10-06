use axum::{
    extract::{Path, State},
    response::sse::{Event, KeepAlive, Sse},
    response::IntoResponse,
};
use serde_json::json;
use std::{convert::Infallible, time::Duration};

use crate::{
    models::{RunEventRecord, RunProgressSnapshotRecord},
    AppState,
};

pub async fn run_events_stream(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> impl IntoResponse {
    let db = state.db.clone();
    let target_run_id = run_id.clone();

    let stream = async_stream::stream! {
        // 1. Fetch current snapshot from SQLite
        if let Ok(Some(snap)) = sqlx::query_as::<_, RunProgressSnapshotRecord>(
            "SELECT * FROM run_progress_snapshots WHERE suite_run_id = ?"
        )
        .bind(&target_run_id)
        .fetch_optional(&db)
        .await
        {
            let payload = json!({
                "run_id": snap.suite_run_id,
                "status": snap.status,
                "progress": serde_json::from_str::<serde_json::Value>(&snap.progress_json).unwrap_or(json!({})),
                "stats": serde_json::from_str::<serde_json::Value>(&snap.stats_json).unwrap_or(json!({})),
                "last_sequence": snap.last_sequence,
                "updated_at": snap.updated_at
            });

            yield Ok::<Event, Infallible>(Event::default()
                .event("run.snapshot")
                .id(snap.last_sequence.to_string())
                .data(payload.to_string()));
        }

        // 2. Poll/stream events for this run
        let mut last_seq = 0i64;
        let mut interval = tokio::time::interval(Duration::from_millis(500));

        loop {
            interval.tick().await;

            let events = sqlx::query_as::<_, RunEventRecord>(
                "SELECT * FROM run_events WHERE suite_run_id = ? AND sequence > ? ORDER BY sequence ASC LIMIT 20"
            )
            .bind(&target_run_id)
            .bind(last_seq)
            .fetch_all(&db)
            .await
            .unwrap_or_default();

            for ev in events {
                last_seq = ev.sequence;
                yield Ok::<Event, Infallible>(Event::default()
                    .event(ev.event_type)
                    .id(ev.sequence.to_string())
                    .data(ev.payload_json));
            }

            // Check if run is in a terminal status
            let is_finished: Option<String> = sqlx::query_scalar(
                "SELECT status FROM suite_runs WHERE id = ?"
            )
            .bind(&target_run_id)
            .fetch_optional(&db)
            .await
            .unwrap_or(None);

            if let Some(status) = is_finished {
                if matches!(status.as_str(), "PASSED" | "FAILED" | "ERROR" | "CANCELED" | "INTERRUPTED") {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    break;
                }
            }
        }
    };

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("heartbeat"),
    )
}
