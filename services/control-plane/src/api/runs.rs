use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{
    models::{
        AssetRevision, ComparisonItem, ProgressInfo, RunComparisonResponse, RunLinks,
        RunProgressSnapshotRecord, RunStatsSummary, StepRunRecord, SuiteRunRecord,
        TriggerRunRequest, TriggerRunResponse,
    },
    AppState,
};

#[derive(Deserialize)]
pub struct ListRunsQuery {
    pub status: Option<String>,
    pub limit: Option<i64>,
}

pub async fn trigger_run(
    State(state): State<AppState>,
    Json(payload): Json<TriggerRunRequest>,
) -> impl IntoResponse {
    let workspace_id = "00000000-0000-0000-0000-000000000001";

    // 1. Check idempotency key if provided
    if let Some(ref idem_key) = payload.idempotency_key {
        let existing = sqlx::query_as::<_, SuiteRunRecord>(
            "SELECT * FROM suite_runs WHERE idempotency_key = ?"
        )
        .bind(idem_key)
        .fetch_optional(&state.db)
        .await;

        if let Ok(Some(run)) = existing {
            return (
                StatusCode::OK,
                Json(json!(TriggerRunResponse {
                    run_id: run.id.clone(),
                    status: run.status,
                    created_at: run.created_at,
                    links: RunLinks {
                        status: format!("/api/v1/runs/{}", run.id),
                        events: format!("/api/v1/runs/{}/events", run.id),
                        report: format!("/api/v1/runs/{}/report", run.id),
                    },
                })),
            );
        }
    }

    // 2. Validate suite_revision_id exists
    let revision = match sqlx::query_as::<_, AssetRevision>(
        "SELECT * FROM asset_revisions WHERE id = ?"
    )
    .bind(&payload.suite_revision_id)
    .fetch_optional(&state.db)
    .await
    {
        Ok(Some(r)) => r,
        Ok(None) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "Suite revision does not exist" })),
            )
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        }
    };

    // 3. Prepare run record
    let run_id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let inputs_str = payload.inputs.map(|v| v.to_string()).unwrap_or_else(|| "{}".to_string());
    let overrides_str = payload
        .variable_overrides
        .map(|v| v.to_string())
        .unwrap_or_else(|| "{}".to_string());
    let random_seed: i64 = rand::random::<u32>() as i64;

    // Run manifest captures pinned revision and parameters
    let run_manifest = json!({
        "suite_revision_id": payload.suite_revision_id,
        "suite_checksum": revision.checksum,
        "environment_id": payload.environment_id,
        "seed": random_seed,
        "catalog_version": "v1"
    });

    let mut tx = match state.db.begin().await {
        Ok(t) => t,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        }
    };

    // 4. Insert SuiteRun in QUEUED status
    let insert_run = sqlx::query(
        "INSERT INTO suite_runs (id, workspace_id, suite_revision_id, environment_id, status, idempotency_key, 
         run_manifest_json, random_seed, catalog_version, inputs_json, variable_overrides_json, created_at)
         VALUES (?, ?, ?, ?, 'QUEUED', ?, ?, ?, 'v1', ?, ?, ?)"
    )
    .bind(&run_id)
    .bind(workspace_id)
    .bind(&payload.suite_revision_id)
    .bind(&payload.environment_id)
    .bind(&payload.idempotency_key)
    .bind(run_manifest.to_string())
    .bind(random_seed)
    .bind(&inputs_str)
    .bind(&overrides_str)
    .bind(&now)
    .execute(&mut *tx)
    .await;

    if let Err(e) = insert_run {
        let _ = tx.rollback().await;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Failed to create run: {}", e) })),
        );
    }

    // 5. Insert initial progress snapshot
    let init_progress = json!({
        "mode": "determinate",
        "percent": 0,
        "terminal_nodes": 0,
        "planned_nodes": 0
    });
    let init_stats = json!({
        "cases_total": 0,
        "cases_passed": 0,
        "cases_failed": 0
    });

    let _ = sqlx::query(
        "INSERT INTO run_progress_snapshots (suite_run_id, status, progress_json, stats_json, last_sequence, updated_at)
         VALUES (?, 'QUEUED', ?, ?, 1, ?)"
    )
    .bind(&run_id)
    .bind(init_progress.to_string())
    .bind(init_stats.to_string())
    .bind(&now)
    .execute(&mut *tx)
    .await;

    // 6. Insert initial RunEvent
    let event_id = Uuid::new_v4().to_string();
    let _ = sqlx::query(
        "INSERT INTO run_events (id, suite_run_id, sequence, event_type, payload_json, occurred_at)
         VALUES (?, ?, 1, 'run.queued', ?, ?)"
    )
    .bind(&event_id)
    .bind(&run_id)
    .bind(json!({ "run_id": run_id, "status": "QUEUED" }).to_string())
    .bind(&now)
    .execute(&mut *tx)
    .await;

    // 7. Insert into NATS Outbox
    let outbox_id = Uuid::new_v4().to_string();
    let subject = format!("automation.v1.{}.runs.{}.stats", workspace_id, run_id);
    let _ = sqlx::query(
        "INSERT INTO nats_event_outbox (id, suite_run_id, sequence, subject, payload_json, status, created_at)
         VALUES (?, ?, 1, ?, ?, 'PENDING', ?)"
    )
    .bind(&outbox_id)
    .bind(&run_id)
    .bind(&subject)
    .bind(json!({
        "schema_version": 1,
        "run_id": run_id,
        "status": "QUEUED",
        "sequence": 1,
        "occurred_at": now
    }).to_string())
    .bind(&now)
    .execute(&mut *tx)
    .await;

    if let Err(e) = tx.commit().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Transaction commit failed: {}", e) })),
        );
    }

    (
        StatusCode::ACCEPTED,
        Json(json!(TriggerRunResponse {
            run_id: run_id.clone(),
            status: "QUEUED".to_string(),
            created_at: now,
            links: RunLinks {
                status: format!("/api/v1/runs/{}", run_id),
                events: format!("/api/v1/runs/{}/events", run_id),
                report: format!("/api/v1/runs/{}/report", run_id),
            },
        })),
    )
}

pub async fn list_runs(
    State(state): State<AppState>,
    Query(query): Query<ListRunsQuery>,
) -> impl IntoResponse {
    let limit = query.limit.unwrap_or(50).clamp(1, 100);

    let runs = if let Some(ref status) = query.status {
        sqlx::query_as::<_, SuiteRunRecord>(
            "SELECT * FROM suite_runs WHERE status = ? ORDER BY created_at DESC LIMIT ?"
        )
        .bind(status)
        .bind(limit)
        .fetch_all(&state.db)
        .await
    } else {
        sqlx::query_as::<_, SuiteRunRecord>(
            "SELECT * FROM suite_runs ORDER BY created_at DESC LIMIT ?"
        )
        .bind(limit)
        .fetch_all(&state.db)
        .await
    };

    match runs {
        Ok(list) => (StatusCode::OK, Json(json!(list))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

pub async fn get_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> impl IntoResponse {
    let run = sqlx::query_as::<_, SuiteRunRecord>(
        "SELECT * FROM suite_runs WHERE id = ?"
    )
    .bind(&run_id)
    .fetch_optional(&state.db)
    .await;

    match run {
        Ok(Some(r)) => {
            // Also fetch case runs
            let cases = sqlx::query_as::<_, crate::models::CaseRunRecord>(
                "SELECT * FROM case_runs WHERE suite_run_id = ? ORDER BY ordinal ASC"
            )
            .bind(&run_id)
            .fetch_all(&state.db)
            .await
            .unwrap_or_default();

            (
                StatusCode::OK,
                Json(json!({
                    "run": r,
                    "cases": cases
                })),
            )
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Run not found" })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

pub async fn get_run_stats(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> impl IntoResponse {
    let snapshot = sqlx::query_as::<_, RunProgressSnapshotRecord>(
        "SELECT * FROM run_progress_snapshots WHERE suite_run_id = ?"
    )
    .bind(&run_id)
    .fetch_optional(&state.db)
    .await;

    match snapshot {
        Ok(Some(snap)) => {
            let progress_val: Value = serde_json::from_str(&snap.progress_json).unwrap_or(json!({}));
            let stats_val: Value = serde_json::from_str(&snap.stats_json).unwrap_or(json!({}));
            (
                StatusCode::OK,
                Json(json!({
                    "run_id": snap.suite_run_id,
                    "status": snap.status,
                    "progress": progress_val,
                    "stats": stats_val,
                    "last_sequence": snap.last_sequence,
                    "updated_at": snap.updated_at
                })),
            )
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Run snapshot not found" })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

pub async fn cancel_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> impl IntoResponse {
    let now = Utc::now().to_rfc3339();
    let res = sqlx::query(
        "UPDATE suite_runs SET status = 'CANCELED', finished_at = ? 
         WHERE id = ? AND status IN ('QUEUED', 'RUNNING')"
    )
    .bind(&now)
    .bind(&run_id)
    .execute(&state.db)
    .await;

    match res {
        Ok(r) => {
            if r.rows_affected() > 0 {
                // Update snapshot
                let _ = sqlx::query(
                    "UPDATE run_progress_snapshots SET status = 'CANCELED', updated_at = ? WHERE suite_run_id = ?"
                )
                .bind(&now)
                .bind(&run_id)
                .execute(&state.db)
                .await;

                (StatusCode::OK, Json(json!({ "status": "CANCELED", "run_id": run_id })))
            } else {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "Run is not in an active or queued state" })),
                )
            }
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

pub async fn rerun_failed(
    State(state): State<AppState>,
    Path(source_run_id): Path<String>,
) -> impl IntoResponse {
    // 1. Fetch source run
    let src = match sqlx::query_as::<_, SuiteRunRecord>(
        "SELECT * FROM suite_runs WHERE id = ?"
    )
    .bind(&source_run_id)
    .fetch_optional(&state.db)
    .await
    {
        Ok(Some(r)) => r,
        _ => return (StatusCode::NOT_FOUND, Json(json!({ "error": "Source run not found" }))),
    };

    let new_run_id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let random_seed: i64 = rand::random::<u32>() as i64;

    let res = sqlx::query(
        "INSERT INTO suite_runs (id, workspace_id, suite_revision_id, environment_id, status, source_run_id,
         run_manifest_json, random_seed, catalog_version, inputs_json, variable_overrides_json, created_at)
         VALUES (?, ?, ?, ?, 'QUEUED', ?, ?, ?, 'v1', ?, ?, ?)"
    )
    .bind(&new_run_id)
    .bind(&src.workspace_id)
    .bind(&src.suite_revision_id)
    .bind(&src.environment_id)
    .bind(&source_run_id)
    .bind(&src.run_manifest_json)
    .bind(random_seed)
    .bind(&src.inputs_json)
    .bind(&src.variable_overrides_json)
    .bind(&now)
    .execute(&state.db)
    .await;

    match res {
        Ok(_) => (
            StatusCode::ACCEPTED,
            Json(json!({
                "run_id": new_run_id,
                "status": "QUEUED",
                "source_run_id": source_run_id,
                "created_at": now
            })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

pub async fn get_run_comparison(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> impl IntoResponse {
    // 1. Fetch current run
    let current_run = match sqlx::query_as::<_, SuiteRunRecord>(
        "SELECT * FROM suite_runs WHERE id = ?"
    )
    .bind(&run_id)
    .fetch_optional(&state.db)
    .await
    {
        Ok(Some(r)) => r,
        _ => return (StatusCode::NOT_FOUND, Json(json!({ "error": "Run not found" }))),
    };

    // 2. Query latest prior PASSED run with same suite_revision_id and environment_id
    let baseline = sqlx::query_as::<_, SuiteRunRecord>(
        "SELECT * FROM suite_runs 
         WHERE suite_revision_id = ? AND environment_id = ? AND status = 'PASSED' AND id != ? AND created_at < ?
         ORDER BY created_at DESC LIMIT 1"
    )
    .bind(&current_run.suite_revision_id)
    .bind(&current_run.environment_id)
    .bind(&run_id)
    .bind(&current_run.created_at)
    .fetch_optional(&state.db)
    .await
    .unwrap_or(None);

    let baseline_run_id = baseline.as_ref().map(|b| b.id.clone());
    let baseline_found = baseline.is_some();
    let reason = if !baseline_found {
        Some("No previous successful baseline run found for this suite revision and environment".to_string())
    } else {
        None
    };

    // Build comparison response
    let response = RunComparisonResponse {
        current_run_id: run_id,
        baseline_run_id,
        baseline_found,
        reason,
        summary_delta: json!({
            "status_diff": current_run.status,
            "baseline_status": baseline.as_ref().map(|b| b.status.clone()).unwrap_or_else(|| "N/A".to_string())
        }),
        case_comparisons: Vec::new(),
    };

    (StatusCode::OK, Json(json!(response)))
}

pub async fn export_run(
    State(state): State<AppState>,
    Path((run_id, format)): Path<(String, String)>,
) -> impl IntoResponse {
    let run = match sqlx::query_as::<_, SuiteRunRecord>(
        "SELECT * FROM suite_runs WHERE id = ?"
    )
    .bind(&run_id)
    .fetch_optional(&state.db)
    .await
    {
        Ok(Some(r)) => r,
        _ => return (StatusCode::NOT_FOUND, [("content-type", "text/plain")], "Run not found".to_string()),
    };

    match format.to_lowercase().as_str() {
        "junit" => {
            let junit_xml = format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="TestIT Suite Run" tests="1" failures="{}" time="1.0">
  <testsuite name="Suite-{}" tests="1" failures="{}" time="1.0">
    <testcase classname="Suite-{}" name="Run-{}" time="1.0">
      {}
    </testcase>
  </testsuite>
</testsuites>"#,
                if run.status == "PASSED" { 0 } else { 1 },
                run.suite_revision_id,
                if run.status == "PASSED" { 0 } else { 1 },
                run.suite_revision_id,
                run.id,
                if run.status != "PASSED" {
                    format!("<failure message=\"Run ended with status {}\"/>", run.status)
                } else {
                    "".to_string()
                }
            );
            (StatusCode::OK, [("content-type", "application/xml")], junit_xml)
        }
        "csv" => {
            let csv = format!(
                "run_id,suite_revision_id,environment_id,status,started_at,finished_at\n{},{},{},{},{},{}\n",
                run.id,
                run.suite_revision_id,
                run.environment_id,
                run.status,
                run.started_at.unwrap_or_default(),
                run.finished_at.unwrap_or_default()
            );
            (StatusCode::OK, [("content-type", "text/csv")], csv)
        }
        "html" | _ => {
            let html = format!(
                r#"<!DOCTYPE html>
<html>
<head><meta charset="UTF-8"><title>TestIT Report - {}</title>
<style>body {{ font-family: sans-serif; padding: 2rem; background: #0f172a; color: #f8fafc; }}
.badge {{ padding: 0.25rem 0.75rem; border-radius: 9999px; font-weight: bold; background: {}; color: white; }}
</style></head>
<body>
  <h1>TestIT Execution Report</h1>
  <p>Run ID: <code>{}</code></p>
  <p>Status: <span class="badge">{}</span></p>
  <p>Environment: <code>{}</code></p>
  <p>Started: {} | Finished: {}</p>
</body>
</html>"#,
                run.id,
                if run.status == "PASSED" { "#10b981" } else { "#ef4444" },
                run.id,
                run.status,
                run.environment_id,
                run.started_at.unwrap_or_default(),
                run.finished_at.unwrap_or_default()
            );
            (StatusCode::OK, [("content-type", "text/html")], html)
        }
    }
}
