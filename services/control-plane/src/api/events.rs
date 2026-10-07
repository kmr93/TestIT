use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::sse::{Event, KeepAlive, Sse},
    response::{IntoResponse, Response},
    Extension, Json,
};
use futures::StreamExt;
use serde_json::{json, Value};
use std::{convert::Infallible, time::Duration};
use tracing::warn;

use crate::{
    api::auth::AuthenticatedUser,
    models::{RunEventRecord, RunProgressSnapshotRecord},
    AppState,
};

pub async fn run_events_stream(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    headers: HeaderMap,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    let workspace_id: Option<String> =
        sqlx::query_scalar("SELECT workspace_id FROM suite_runs WHERE id = ?")
            .bind(&run_id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();
    if workspace_id.as_deref() != Some(user.workspace_id.as_str()) {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Run not found" })),
        )
            .into_response();
    }

    let resume_from = match headers.get("last-event-id") {
        Some(value) => {
            match value
                .to_str()
                .ok()
                .and_then(|value| value.parse::<i64>().ok())
            {
                Some(sequence) if sequence >= 0 => Some(sequence),
                _ => {
                    return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "Last-Event-ID must be a non-negative sequence number" })),
                )
                    .into_response();
                }
            }
        }
        None => None,
    };

    let db = state.db.clone();
    let target_run_id = run_id;
    let target_workspace_id = user.workspace_id.clone();
    let nats_client = state.nats_sse.clone();
    let stream = async_stream::stream! {
        let subject = format!("automation.v1.{}.runs.{}.stats", target_workspace_id, target_run_id);
        let mut subscriber = if let Some(client) = nats_client {
            match client.subscribe(subject).await {
                Ok(subscriber) => Some(subscriber),
                Err(error) => {
                    warn!("Could not subscribe to the run-stat subject; using SQLite polling: {}", error);
                    None
                }
            }
        } else {
            None
        };
        let mut transport_mode = if subscriber.is_some() { "nats" } else { "sqlite_polling" };
        yield Ok::<Event, Infallible>(Event::default()
            .event("run.transport")
            .data(json!({ "mode": transport_mode }).to_string()));

        // A new client can start from the latest snapshot. A reconnecting client
        // must receive every durable event after its cursor instead.
        let mut last_seq = if let Some(cursor) = resume_from {
            cursor
        } else {
            if let Ok(Some(snapshot)) = sqlx::query_as::<_, RunProgressSnapshotRecord>(
                "SELECT * FROM run_progress_snapshots WHERE suite_run_id = ?"
            )
            .bind(&target_run_id)
            .fetch_optional(&db)
            .await
            {
                let payload = json!({
                    "run_id": snapshot.suite_run_id,
                    "status": snapshot.status,
                    "progress": serde_json::from_str::<Value>(&snapshot.progress_json).unwrap_or(json!({})),
                    "stats": serde_json::from_str::<Value>(&snapshot.stats_json).unwrap_or(json!({})),
                    "last_sequence": snapshot.last_sequence,
                    "updated_at": snapshot.updated_at,
                    "event_transport": transport_mode
                });
                yield Ok::<Event, Infallible>(Event::default()
                    .event("run.snapshot")
                    .id(snapshot.last_sequence.to_string())
                    .data(payload.to_string()));
                snapshot.last_sequence
            } else {
                0
            }
        };

        let mut interval = tokio::time::interval(Duration::from_millis(500));
        loop {
            let (should_poll, subscriber_closed) = if let Some(nats_subscriber) = subscriber.as_mut() {
                tokio::select! {
                    _ = interval.tick() => (true, false),
                    message = nats_subscriber.next() => match message {
                        Some(message) => (valid_run_event_message(&message.payload, &target_run_id), false),
                        None => (true, true),
                    }
                }
            } else {
                interval.tick().await;
                (true, false)
            };
            if subscriber_closed {
                subscriber = None;
                transport_mode = "sqlite_polling";
                yield Ok::<Event, Infallible>(Event::default()
                    .event("run.transport")
                    .data(json!({ "mode": transport_mode }).to_string()));
            }
            if !should_poll {
                continue;
            }

            let events = sqlx::query_as::<_, RunEventRecord>(
                "SELECT * FROM run_events WHERE suite_run_id = ? AND sequence > ? ORDER BY sequence ASC LIMIT 100"
            )
            .bind(&target_run_id)
            .bind(last_seq)
            .fetch_all(&db)
            .await
            .unwrap_or_default();
            let received_events = !events.is_empty();

            for event in events {
                last_seq = event.sequence;
                yield Ok::<Event, Infallible>(Event::default()
                    .event(event.event_type)
                    .id(event.sequence.to_string())
                    .data(event.payload_json));
            }

            let state = sqlx::query_as::<_, (String, i64)>(
                "SELECT r.status, COALESCE(s.last_sequence, 0)
                 FROM suite_runs r LEFT JOIN run_progress_snapshots s ON s.suite_run_id = r.id
                 WHERE r.id = ?"
            )
            .bind(&target_run_id)
            .fetch_optional(&db)
            .await
            .unwrap_or(None);

            if let Some((status, latest_sequence)) = state {
                let terminal = matches!(status.as_str(), "PASSED" | "FAILED" | "ERROR" | "CANCELED" | "INTERRUPTED");
                if terminal && !received_events && last_seq >= latest_sequence {
                    break;
                }
            } else {
                break;
            }
        }
    };

    Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("heartbeat"),
        )
        .into_response()
}

fn valid_run_event_message(payload: &[u8], run_id: &str) -> bool {
    if payload.len() > 64 * 1024 {
        return false;
    }
    let Ok(message) = serde_json::from_slice::<Value>(payload) else {
        return false;
    };
    message.get("schema_version").and_then(Value::as_i64) == Some(1)
        && message.get("run_id").and_then(Value::as_str) == Some(run_id)
        && message
            .get("sequence")
            .and_then(Value::as_i64)
            .is_some_and(|sequence| sequence > 0)
}

#[cfg(test)]
mod tests {
    use super::valid_run_event_message;

    #[test]
    fn accepts_wakeup_for_expected_run() {
        let payload = br#"{"schema_version":1,"run_id":"run-1","sequence":4}"#;
        assert!(valid_run_event_message(payload, "run-1"));
    }

    #[test]
    fn rejects_malformed_or_unexpected_wakeups() {
        assert!(!valid_run_event_message(b"not json", "run-1"));
        assert!(!valid_run_event_message(
            br#"{"schema_version":2,"run_id":"run-1","sequence":4}"#,
            "run-1"
        ));
        assert!(!valid_run_event_message(
            br#"{"schema_version":1,"run_id":"other-run","sequence":4}"#,
            "run-1"
        ));
        assert!(!valid_run_event_message(
            br#"{"schema_version":1,"run_id":"run-1","sequence":0}"#,
            "run-1"
        ));
        assert!(!valid_run_event_message(
            br#"{"schema_version":1,"run_id":"run-1"}"#,
            "run-1"
        ));
    }

    #[test]
    fn rejects_oversized_wakeups() {
        let payload = vec![b' '; 64 * 1024 + 1];
        assert!(!valid_run_event_message(&payload, "run-1"));
    }
}
