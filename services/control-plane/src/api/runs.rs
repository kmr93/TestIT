use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{
    api::auth::AuthenticatedUser,
    models::{
        AssetRevision, ComparisonItem, RunComparisonResponse, RunLinks, RunProgressSnapshotRecord,
        StepRunRecord, SuiteRunRecord, TriggerRunRequest, TriggerRunResponse,
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
    Extension(user): Extension<AuthenticatedUser>,
    Json(payload): Json<TriggerRunRequest>,
) -> impl IntoResponse {
    for (name, value) in [
        ("run inputs", payload.inputs.as_ref()),
        ("variable overrides", payload.variable_overrides.as_ref()),
    ] {
        if let Some(value) = value {
            if !value.is_object()
                || serde_json::to_vec(value).map_or(true, |bytes| bytes.len() > 65_536)
                || contains_credential_key(value)
            {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(
                        json!({ "error": format!("{} must be an object up to 64 KiB and cannot contain credential fields", name) }),
                    ),
                );
            }
        }
    }
    if !super::health::worker_engine_available(&state).await {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(
                json!({ "error": "The isolated worker engine is unavailable; run was not queued" }),
            ),
        );
    }
    let workspace_id = user.workspace_id.clone();

    // 1. Check idempotency key if provided
    let scoped_idempotency_key = payload
        .idempotency_key
        .as_deref()
        .map(|key| format!("{}:{}", workspace_id, key));
    if let Some(ref idem_key) = scoped_idempotency_key {
        let existing = sqlx::query_as::<_, SuiteRunRecord>(
            "SELECT * FROM suite_runs WHERE idempotency_key = ? AND workspace_id = ?",
        )
        .bind(idem_key)
        .bind(&workspace_id)
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
        "SELECT revisions.* FROM asset_revisions revisions
             JOIN assets ON assets.id = revisions.asset_id
             WHERE revisions.id = ? AND assets.workspace_id = ? AND assets.kind = 'suite'",
    )
    .bind(&payload.suite_revision_id)
    .bind(&workspace_id)
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

    let suite_definition =
        serde_json::from_str::<Value>(&revision.definition_json).unwrap_or(Value::Null);
    let (resource_locks, lock_wait_timeout_seconds) =
        match crate::resource_locks::validate_suite_resource_locks(&suite_definition) {
            Ok(lock_settings) => lock_settings,
            Err(message) => {
                return (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(json!({ "error": message, "code": "RESOURCE_LOCKS_INVALID" })),
                )
            }
        };
    let run_inputs = payload
        .inputs
        .as_ref()
        .cloned()
        .unwrap_or_else(|| json!({}));
    if let Err(message) =
        crate::variables::validate_run_inputs(suite_definition.get("input_schema"), &run_inputs)
    {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": message, "code": "RUN_INPUTS_INVALID" })),
        );
    }

    let environment_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM environments WHERE id = ? AND workspace_id = ?)",
    )
    .bind(&payload.environment_id)
    .bind(&workspace_id)
    .fetch_one(&state.db)
    .await
    .unwrap_or(false);
    if !environment_exists {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Environment does not exist in this workspace" })),
        );
    }

    // 3. Prepare run record
    let run_id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let inputs_str = payload
        .inputs
        .map(|v| v.to_string())
        .unwrap_or_else(|| "{}".to_string());
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
        "resource_locks": resource_locks,
        "lock_wait_timeout_seconds": lock_wait_timeout_seconds,
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
        "INSERT INTO suite_runs (id, workspace_id, suite_revision_id, environment_id, status, initiating_user_id, idempotency_key,
         run_manifest_json, random_seed, catalog_version, inputs_json, variable_overrides_json, created_at)
         VALUES (?, ?, ?, ?, 'QUEUED', ?, ?, ?, ?, 'v1', ?, ?, ?)"
    )
    .bind(&run_id)
    .bind(&workspace_id)
    .bind(&payload.suite_revision_id)
    .bind(&payload.environment_id)
    .bind(&user.id)
    .bind(&scoped_idempotency_key)
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
         VALUES (?, ?, 1, 'run.queued', ?, ?)",
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
    Extension(user): Extension<AuthenticatedUser>,
    Query(query): Query<ListRunsQuery>,
) -> impl IntoResponse {
    let limit = query.limit.unwrap_or(50).clamp(1, 100);

    let runs = if let Some(ref status) = query.status {
        sqlx::query_as::<_, SuiteRunRecord>(
            "SELECT * FROM suite_runs WHERE workspace_id = ? AND status = ? ORDER BY created_at DESC LIMIT ?",
        )
        .bind(&user.workspace_id)
        .bind(status)
        .bind(limit)
        .fetch_all(&state.db)
        .await
    } else {
        sqlx::query_as::<_, SuiteRunRecord>(
            "SELECT * FROM suite_runs WHERE workspace_id = ? ORDER BY created_at DESC LIMIT ?",
        )
        .bind(&user.workspace_id)
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
    Extension(user): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let run = sqlx::query_as::<_, SuiteRunRecord>(
        "SELECT * FROM suite_runs WHERE id = ? AND workspace_id = ?",
    )
    .bind(&run_id)
    .bind(&user.workspace_id)
    .fetch_optional(&state.db)
    .await;

    match run {
        Ok(Some(r)) => {
            // Also fetch case runs
            let cases = sqlx::query_as::<_, crate::models::CaseRunRecord>(
                "SELECT * FROM case_runs WHERE suite_run_id = ? ORDER BY ordinal ASC",
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
    Extension(user): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let snapshot = sqlx::query_as::<_, RunProgressSnapshotRecord>(
        "SELECT snapshots.* FROM run_progress_snapshots snapshots
         JOIN suite_runs ON suite_runs.id = snapshots.suite_run_id
         WHERE snapshots.suite_run_id = ? AND suite_runs.workspace_id = ?",
    )
    .bind(&run_id)
    .bind(&user.workspace_id)
    .fetch_optional(&state.db)
    .await;

    match snapshot {
        Ok(Some(snap)) => {
            let progress_val: Value =
                serde_json::from_str(&snap.progress_json).unwrap_or(json!({}));
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
    Extension(user): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let Some((workspace_id, current_status)) = sqlx::query_as::<_, (String, String)>(
        "SELECT workspace_id, status FROM suite_runs WHERE id = ?",
    )
    .bind(&run_id)
    .fetch_optional(&state.db)
    .await
    .unwrap_or(None) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Run not found" })),
        );
    };
    if workspace_id != user.workspace_id {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Run not found" })),
        );
    }
    if !matches!(current_status.as_str(), "QUEUED" | "RUNNING") {
        return (
            StatusCode::CONFLICT,
            Json(json!({ "error": "Run is not in an active or queued state" })),
        );
    }
    if current_status == "RUNNING" {
        let (Some(url), Some(token)) = (
            state.config.worker_manager_url.as_deref(),
            state.config.worker_manager_token.as_deref(),
        ) else {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(
                    json!({ "error": "The worker engine is unavailable; cancellation was not confirmed" }),
                ),
            );
        };
        let cancellation = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .ok()
            .and_then(|client| Some((client, url, token)));
        let Some((client, url, token)) = cancellation else {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(
                    json!({ "error": "The worker engine is unavailable; cancellation was not confirmed" }),
                ),
            );
        };
        match client
            .post(format!("{}/v1/cancel", url))
            .bearer_auth(token)
            .json(&json!({ "run_id": run_id }))
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => {}
            _ => {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({ "error": "The worker engine could not confirm cancellation" })),
                );
            }
        }
    }

    let now = Utc::now().to_rfc3339();
    let mut tx = match state.db.begin().await {
        Ok(tx) => tx,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": error.to_string() })),
            )
        }
    };
    let updated = sqlx::query(
        "UPDATE suite_runs SET status = 'CANCELED', finished_at = ? WHERE id = ? AND status = ?",
    )
    .bind(&now)
    .bind(&run_id)
    .bind(&current_status)
    .execute(&mut *tx)
    .await;
    match updated {
        Ok(result) if result.rows_affected() == 1 => {}
        Ok(_) => {
            let _ = tx.rollback().await;
            return (
                StatusCode::CONFLICT,
                Json(json!({ "error": "Run state changed during cancellation" })),
            );
        }
        Err(error) => {
            let _ = tx.rollback().await;
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": error.to_string() })),
            );
        }
    }
    let sequence: i64 = match sqlx::query_scalar(
        "SELECT COALESCE(MAX(sequence), 0) + 1 FROM run_events WHERE suite_run_id = ?",
    )
    .bind(&run_id)
    .fetch_one(&mut *tx)
    .await
    {
        Ok(sequence) => sequence,
        Err(error) => {
            let _ = tx.rollback().await;
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": error.to_string() })),
            );
        }
    };
    let payload = json!({
        "schema_version": 1,
        "run_id": run_id,
        "sequence": sequence,
        "event_type": "run.finished",
        "status": "CANCELED",
        "occurred_at": now
    });
    let event_id = Uuid::new_v4().to_string();
    let insert_event = sqlx::query(
        "INSERT INTO run_events (id, suite_run_id, sequence, event_type, payload_json, occurred_at) VALUES (?, ?, ?, 'run.finished', ?, ?)",
    )
    .bind(&event_id)
    .bind(&run_id)
    .bind(sequence)
    .bind(payload.to_string())
    .bind(&now)
    .execute(&mut *tx)
    .await;
    if let Err(error) = insert_event {
        let _ = tx.rollback().await;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        );
    }
    let outbox_id = Uuid::new_v4().to_string();
    let subject = format!("automation.v1.{}.runs.{}.stats", workspace_id, run_id);
    let outbox = sqlx::query(
        "INSERT INTO nats_event_outbox (id, suite_run_id, sequence, subject, payload_json, status, created_at) VALUES (?, ?, ?, ?, ?, 'PENDING', ?)",
    )
    .bind(&outbox_id)
    .bind(&run_id)
    .bind(sequence)
    .bind(subject)
    .bind(payload.to_string())
    .bind(&now)
    .execute(&mut *tx)
    .await;
    if let Err(error) = outbox {
        let _ = tx.rollback().await;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        );
    }
    let snapshot = sqlx::query(
        "UPDATE run_progress_snapshots SET status = 'CANCELED', last_sequence = ?, updated_at = ? WHERE suite_run_id = ?",
    )
    .bind(sequence)
    .bind(&now)
    .bind(&run_id)
    .execute(&mut *tx)
    .await;
    if let Err(error) = snapshot {
        let _ = tx.rollback().await;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        );
    }
    let audit = sqlx::query(
        "INSERT INTO audit_events (id, actor_id, action, target_type, target_id, changes_json, created_at) VALUES (?, ?, 'run.cancel', 'suite_run', ?, '{}', ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(&user.id)
    .bind(&run_id)
    .bind(&now)
    .execute(&mut *tx)
    .await;
    if let Err(error) = audit {
        let _ = tx.rollback().await;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        );
    }
    if let Err(error) = tx.commit().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        );
    }
    (
        StatusCode::OK,
        Json(json!({ "status": "CANCELED", "run_id": run_id })),
    )
}

pub async fn rerun_failed(
    State(state): State<AppState>,
    Path(source_run_id): Path<String>,
    Extension(user): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    // 1. Fetch source run
    let src = match sqlx::query_as::<_, SuiteRunRecord>(
        "SELECT * FROM suite_runs WHERE id = ? AND workspace_id = ?",
    )
    .bind(&source_run_id)
    .bind(&user.workspace_id)
    .fetch_optional(&state.db)
    .await
    {
        Ok(Some(r)) => r,
        _ => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "Source run not found" })),
            )
        }
    };

    if matches!(src.status.as_str(), "QUEUED" | "RUNNING") {
        return (
            StatusCode::CONFLICT,
            Json(json!({ "error": "Failed cases can be rerun after the source run finishes" })),
        );
    }
    let failed_case_runs = sqlx::query_as::<_, crate::models::CaseRunRecord>(
        "SELECT * FROM case_runs WHERE suite_run_id = ? AND execution_scope = 'case' AND status IN ('FAILED', 'ERROR') ORDER BY ordinal",
    )
    .bind(&source_run_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();
    if failed_case_runs.is_empty() {
        return (
            StatusCode::CONFLICT,
            Json(json!({ "error": "The source run has no failed or errored case iterations" })),
        );
    }
    let failed_revisions = failed_case_runs
        .iter()
        .map(|case| case.case_revision_id.clone())
        .collect::<std::collections::HashSet<_>>();
    let failed_revisions = failed_revisions.into_iter().collect::<Vec<_>>();
    let failed_iterations = failed_case_runs
        .iter()
        .map(|case| {
            json!({
                "case_revision_id": case.case_revision_id,
                "iteration_index": case.iteration_index
            })
        })
        .collect::<Vec<_>>();

    let new_run_id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let random_seed = src.random_seed;
    let mut manifest: Value = serde_json::from_str(&src.run_manifest_json).unwrap_or(json!({}));
    if let Some(object) = manifest.as_object_mut() {
        object.insert("source_run_id".into(), json!(source_run_id));
        object.insert("rerun_case_revision_ids".into(), json!(failed_revisions));
        object.insert("rerun_failed_iterations".into(), json!(failed_iterations));
    }

    let mut tx = match state.db.begin().await {
        Ok(tx) => tx,
        Err(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": "Run service is temporarily unavailable" })),
            )
        }
    };
    let res = sqlx::query(
        "INSERT INTO suite_runs (id, workspace_id, suite_revision_id, environment_id, status, initiating_user_id, source_run_id,
         run_manifest_json, random_seed, catalog_version, inputs_json, variable_overrides_json, created_at)
         VALUES (?, ?, ?, ?, 'QUEUED', ?, ?, ?, ?, 'v1', ?, ?, ?)"
    )
    .bind(&new_run_id)
    .bind(&src.workspace_id)
    .bind(&src.suite_revision_id)
    .bind(&src.environment_id)
    .bind(&user.id)
    .bind(&source_run_id)
    .bind(manifest.to_string())
    .bind(random_seed)
    .bind(&src.inputs_json)
    .bind(&src.variable_overrides_json)
    .bind(&now)
    .execute(&mut *tx)
    .await;
    if res.is_err() {
        let _ = tx.rollback().await;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Could not queue the failed cases for rerun" })),
        );
    }
    let initial_progress = json!({ "mode": "determinate", "percent": 0, "terminal_nodes": 0, "planned_nodes": failed_revisions.len() });
    let initial_stats =
        json!({ "cases_total": failed_revisions.len(), "cases_passed": 0, "cases_failed": 0 });
    let event_payload = json!({ "schema_version": 1, "run_id": new_run_id, "status": "QUEUED", "sequence": 1, "event_type": "run.queued", "occurred_at": now });
    if sqlx::query("INSERT INTO run_progress_snapshots (suite_run_id, status, progress_json, stats_json, last_sequence, updated_at) VALUES (?, 'QUEUED', ?, ?, 1, ?)")
        .bind(&new_run_id).bind(initial_progress.to_string()).bind(initial_stats.to_string()).bind(&now).execute(&mut *tx).await.is_err()
        || sqlx::query("INSERT INTO run_events (id, suite_run_id, sequence, event_type, payload_json, occurred_at) VALUES (?, ?, 1, 'run.queued', ?, ?)")
            .bind(Uuid::new_v4().to_string()).bind(&new_run_id).bind(event_payload.to_string()).bind(&now).execute(&mut *tx).await.is_err()
        || sqlx::query("INSERT INTO nats_event_outbox (id, suite_run_id, sequence, subject, payload_json, status, created_at) VALUES (?, ?, 1, ?, ?, 'PENDING', ?)")
            .bind(Uuid::new_v4().to_string()).bind(&new_run_id).bind(format!("automation.v1.{}.runs.{}.stats", src.workspace_id, new_run_id)).bind(event_payload.to_string()).bind(&now).execute(&mut *tx).await.is_err()
        || sqlx::query("INSERT INTO audit_events (id, actor_id, action, target_type, target_id, changes_json, created_at) VALUES (?, ?, 'run.rerun_failed', 'suite_run', ?, ?, ?)")
            .bind(Uuid::new_v4().to_string()).bind(&user.id).bind(&new_run_id).bind(json!({ "source_run_id": source_run_id, "case_revision_ids": failed_revisions }).to_string()).bind(&now).execute(&mut *tx).await.is_err()
    {
        let _ = tx.rollback().await;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Could not initialize the failed-case rerun" })),
        );
    }
    if tx.commit().await.is_err() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Could not save the failed-case rerun" })),
        );
    }
    (
        StatusCode::ACCEPTED,
        Json(
            json!({ "run_id": new_run_id, "status": "QUEUED", "source_run_id": source_run_id, "created_at": now }),
        ),
    )
}

pub async fn get_run_comparison(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Extension(user): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    // 1. Fetch current run
    let current_run = match sqlx::query_as::<_, SuiteRunRecord>(
        "SELECT * FROM suite_runs WHERE id = ? AND workspace_id = ?",
    )
    .bind(&run_id)
    .bind(&user.workspace_id)
    .fetch_optional(&state.db)
    .await
    {
        Ok(Some(r)) => r,
        _ => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "Run not found" })),
            )
        }
    };

    // 2. Query latest prior PASSED run with same suite_revision_id and environment_id
    let baseline = sqlx::query_as::<_, SuiteRunRecord>(
        "SELECT * FROM suite_runs 
         WHERE workspace_id = ? AND suite_revision_id = ? AND environment_id = ? AND status = 'PASSED' AND id != ? AND created_at < ?
         ORDER BY created_at DESC LIMIT 1"
    )
    .bind(&user.workspace_id)
    .bind(&current_run.suite_revision_id)
    .bind(&current_run.environment_id)
    .bind(&run_id)
    .bind(&current_run.created_at)
    .fetch_optional(&state.db)
    .await
    .unwrap_or(None);

    let current_cases = load_comparison_cases(&state, &run_id).await;
    let baseline_cases = match baseline.as_ref() {
        Some(run) => load_comparison_cases(&state, &run.id).await,
        None => Vec::new(),
    };
    let baseline_by_key = baseline_cases
        .into_iter()
        .map(|case| ((case.0.clone(), case.1), case))
        .collect::<std::collections::HashMap<_, _>>();
    let case_comparisons = current_cases
        .into_iter()
        .map(
            |(revision_id, iteration, name, status, started, finished)| {
                let previous = baseline_by_key.get(&(revision_id, iteration));
                let baseline_duration =
                    previous.and_then(|case| elapsed_ms(case.4.as_deref(), case.5.as_deref()));
                let current_duration = elapsed_ms(started.as_deref(), finished.as_deref());
                ComparisonItem {
                    name: format!("{} · iteration {}", name, iteration + 1),
                    baseline_status: previous.map(|case| case.3.clone()),
                    current_status: status,
                    baseline_duration_ms: baseline_duration,
                    current_duration_ms: current_duration,
                    duration_delta_ms: baseline_duration
                        .zip(current_duration)
                        .map(|(old, new)| new - old),
                }
            },
        )
        .collect();

    let baseline_run_id = baseline.as_ref().map(|b| b.id.clone());
    let baseline_found = baseline.is_some();
    let reason = if !baseline_found {
        Some(
            "No previous successful baseline run found for this suite revision and environment"
                .to_string(),
        )
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
        case_comparisons,
    };

    (StatusCode::OK, Json(json!(response)))
}

pub async fn export_run(
    State(state): State<AppState>,
    Path((run_id, format)): Path<(String, String)>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    let run = match sqlx::query_as::<_, SuiteRunRecord>(
        "SELECT * FROM suite_runs WHERE id = ? AND workspace_id = ?",
    )
    .bind(&run_id)
    .bind(&user.workspace_id)
    .fetch_optional(&state.db)
    .await
    {
        Ok(Some(r)) => r,
        _ => {
            return (
                StatusCode::NOT_FOUND,
                [("content-type", "text/plain")],
                "Run not found".to_string(),
            )
                .into_response()
        }
    };

    let case_rows = sqlx::query_as::<_, crate::models::CaseRunRecord>(
        "SELECT * FROM case_runs WHERE suite_run_id = ? ORDER BY ordinal, iteration_index",
    )
    .bind(&run_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();
    let steps = sqlx::query_as::<_, StepRunRecord>(
        "SELECT steps.* FROM step_runs steps JOIN case_runs cases ON cases.id = steps.case_run_id
         WHERE cases.suite_run_id = ? ORDER BY cases.ordinal, cases.iteration_index, steps.ordinal, steps.attempt",
    )
    .bind(&run_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();
    let case_names = load_case_names(&state, &case_rows).await;
    let mut steps_by_case = std::collections::HashMap::<String, Vec<&StepRunRecord>>::new();
    for step in &steps {
        steps_by_case
            .entry(step.case_run_id.clone())
            .or_default()
            .push(step);
    }

    match format.to_lowercase().as_str() {
        "junit" => {
            let tests = if case_rows.is_empty() {
                1
            } else {
                case_rows
                    .iter()
                    .map(|case| steps_by_case.get(&case.id).map_or(1, Vec::len))
                    .sum()
            };
            let failures = steps
                .iter()
                .filter(|step| !matches!(step.status.as_str(), "SUCCEEDED" | "SKIPPED"))
                .count()
                + case_rows
                    .iter()
                    .filter(|case| {
                        !matches!(case.status.as_str(), "PASSED" | "SKIPPED")
                            && steps_by_case.get(&case.id).is_none_or(Vec::is_empty)
                    })
                    .count()
                + if case_rows.is_empty() && run.status != "PASSED" {
                    1
                } else {
                    0
                };
            let mut xml = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuites><testsuite name=\"TestIT Run {}\" tests=\"{}\" failures=\"{}\">", xml_escape(&run.id), tests, failures);
            for case in &case_rows {
                let name = report_case_name(case, &case_names);
                if let Some(case_steps) = steps_by_case.get(&case.id) {
                    for step in case_steps {
                        let duration = step.duration_ms.unwrap_or_default() / 1000.0;
                        let test_name = if case.execution_scope == "case" {
                            format!("Iteration {}: {}", case.iteration_index + 1, step.node_name)
                        } else {
                            step.node_name.clone()
                        };
                        xml.push_str(&format!(
                            "<testcase classname=\"{}\" name=\"{}\" time=\"{:.3}\">",
                            xml_escape(&name),
                            xml_escape(&test_name),
                            duration
                        ));
                        if step.status == "SKIPPED" {
                            xml.push_str("<skipped/>");
                        } else if step.status != "SUCCEEDED" {
                            let message = step.error_json.as_deref().unwrap_or(&step.status);
                            xml.push_str(&format!(
                                "<failure message=\"{}\">{}</failure>",
                                xml_escape(&step.status),
                                xml_escape(message)
                            ));
                        }
                        xml.push_str("</testcase>");
                    }
                } else {
                    let failed = case.status != "PASSED";
                    xml.push_str(&format!(
                        "<testcase classname=\"{}\" name=\"{}\">{}</testcase>",
                        xml_escape(&name),
                        xml_escape(&name),
                        if case.status == "SKIPPED" {
                            "<skipped/>".to_string()
                        } else if failed {
                            format!("<failure message=\"{}\"/>", xml_escape(&case.status))
                        } else {
                            String::new()
                        }
                    ));
                }
            }
            if case_rows.is_empty() {
                xml.push_str(&format!(
                    "<testcase classname=\"TestIT\" name=\"{}\">{} </testcase>",
                    xml_escape(&run.id),
                    if run.status == "PASSED" {
                        String::new()
                    } else {
                        format!("<failure message=\"{}\"/>", xml_escape(&run.status))
                    }
                ));
            }
            xml.push_str("</testsuite></testsuites>");
            report_response("application/xml; charset=utf-8", xml, false)
        }
        "csv" => {
            let mut csv = String::from(
                "run_id,case_name,iteration,case_status,step_name,step_status,duration_ms,error\n",
            );
            for case in &case_rows {
                let name = report_case_name(case, &case_names);
                if let Some(case_steps) = steps_by_case.get(&case.id) {
                    for step in case_steps {
                        csv.push_str(&format!(
                            "{},{},{},{},{},{},{},{}\n",
                            csv_escape(&run.id),
                            csv_escape(&name),
                            if case.execution_scope == "case" {
                                (case.iteration_index + 1).to_string()
                            } else {
                                String::new()
                            },
                            csv_escape(&case.status),
                            csv_escape(&step.node_name),
                            csv_escape(&step.status),
                            step.duration_ms.unwrap_or_default(),
                            csv_escape(step.error_json.as_deref().unwrap_or(""))
                        ));
                    }
                } else {
                    csv.push_str(&format!(
                        "{},{},{},{},,, ,\n",
                        csv_escape(&run.id),
                        csv_escape(&name),
                        if case.execution_scope == "case" {
                            (case.iteration_index + 1).to_string()
                        } else {
                            String::new()
                        },
                        csv_escape(&case.status)
                    ));
                }
            }
            if case_rows.is_empty() {
                csv.push_str(&format!(
                    "{},{},,{},,,,\n",
                    csv_escape(&run.id),
                    csv_escape("Suite"),
                    csv_escape(&run.status)
                ));
            }
            report_response("text/csv; charset=utf-8", csv, false)
        }
        "html" | _ => {
            let color = if run.status == "PASSED" {
                "#10b981"
            } else {
                "#ef4444"
            };
            let mut html = format!("<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>TestIT run {}</title><style>body{{font-family:system-ui,sans-serif;max-width:1100px;margin:3rem auto;padding:0 2rem;background:#0f172a;color:#f8fafc}}table{{width:100%;border-collapse:collapse;margin-top:1rem}}th,td{{text-align:left;padding:.7rem;border-bottom:1px solid #334155}}.badge{{color:{};font-weight:700}}pre{{white-space:pre-wrap;overflow-wrap:anywhere;color:#fca5a5}}</style></head><body><h1>Test run report</h1><p>Run ID: <code>{}</code></p><p>Status: <span class=\"badge\">{}</span></p><p>Started: {} · Finished: {}</p><h2>Results</h2><table><thead><tr><th>Case</th><th>Iteration</th><th>Case status</th><th>Step</th><th>Step status</th><th>Duration</th></tr></thead><tbody>", xml_escape(&run.id), color, xml_escape(&run.id), xml_escape(&run.status), xml_escape(run.started_at.as_deref().unwrap_or("")), xml_escape(run.finished_at.as_deref().unwrap_or("")));
            for case in &case_rows {
                let name = report_case_name(case, &case_names);
                let iteration = if case.execution_scope == "case" {
                    (case.iteration_index + 1).to_string()
                } else {
                    String::new()
                };
                if let Some(case_steps) = steps_by_case.get(&case.id) {
                    for step in case_steps {
                        html.push_str(&format!("<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{:.1} ms</td></tr>", xml_escape(&name), xml_escape(&iteration), xml_escape(&case.status), xml_escape(&step.node_name), xml_escape(&step.status), step.duration_ms.unwrap_or_default()));
                        if let Some(error) = &step.error_json {
                            html.push_str(&format!(
                                "<tr><td colspan=\"6\"><pre>{}</pre></td></tr>",
                                xml_escape(error)
                            ));
                        }
                    }
                } else {
                    html.push_str(&format!("<tr><td>{}</td><td>{}</td><td>{}</td><td colspan=\"3\">No step details recorded</td></tr>", xml_escape(&name), xml_escape(&iteration), xml_escape(&case.status)));
                }
            }
            html.push_str("</tbody></table></body></html>");
            report_response("text/html; charset=utf-8", html, true)
        }
    }
}

async fn load_comparison_cases(
    state: &AppState,
    run_id: &str,
) -> Vec<(String, i64, String, String, Option<String>, Option<String>)> {
    sqlx::query_as(
        "SELECT case_runs.case_revision_id, case_runs.iteration_index, assets.name, case_runs.status, case_runs.started_at, case_runs.finished_at
         FROM case_runs JOIN asset_revisions ON asset_revisions.id = case_runs.case_revision_id
         JOIN assets ON assets.id = asset_revisions.asset_id
         WHERE case_runs.suite_run_id = ? AND case_runs.execution_scope = 'case' ORDER BY case_runs.ordinal, case_runs.iteration_index",
    )
    .bind(run_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default()
}

async fn load_case_names(
    state: &AppState,
    cases: &[crate::models::CaseRunRecord],
) -> std::collections::HashMap<String, String> {
    let mut names = std::collections::HashMap::new();
    for case in cases {
        if names.contains_key(&case.case_revision_id) {
            continue;
        }
        if let Ok(Some(name)) = sqlx::query_scalar::<_, String>(
            "SELECT assets.name FROM asset_revisions JOIN assets ON assets.id = asset_revisions.asset_id WHERE asset_revisions.id = ?",
        )
        .bind(&case.case_revision_id)
        .fetch_optional(&state.db)
        .await {
            names.insert(case.case_revision_id.clone(), name);
        }
    }
    names
}

fn report_case_name(
    case: &crate::models::CaseRunRecord,
    names: &std::collections::HashMap<String, String>,
) -> String {
    match case.execution_scope.as_str() {
        "suite_setup" => "Suite setup".to_string(),
        "suite_cleanup" => "Suite cleanup".to_string(),
        _ => names
            .get(&case.case_revision_id)
            .cloned()
            .unwrap_or_else(|| "Test case".to_string()),
    }
}

fn elapsed_ms(started: Option<&str>, finished: Option<&str>) -> Option<f64> {
    let start = chrono::DateTime::parse_from_rfc3339(started?).ok()?;
    let finish = chrono::DateTime::parse_from_rfc3339(finished?).ok()?;
    Some((finish - start).num_milliseconds().max(0) as f64)
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn csv_escape(value: &str) -> String {
    let trimmed = value.trim_start();
    let safe = if trimmed
        .chars()
        .next()
        .is_some_and(|ch| matches!(ch, '=' | '+' | '-' | '@'))
    {
        format!("'{}", value)
    } else {
        value.to_string()
    };
    format!("\"{}\"", safe.replace('"', "\"\""))
}

fn contains_credential_key(value: &Value) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(key, value)| {
            let key = key.to_ascii_lowercase();
            [
                "password",
                "secret",
                "token",
                "authorization",
                "cookie",
                "credential",
            ]
            .iter()
            .any(|marker| key.contains(marker))
                || contains_credential_key(value)
        }),
        Value::Array(values) => values.iter().any(contains_credential_key),
        _ => false,
    }
}

fn report_response(content_type: &'static str, body: String, html: bool) -> Response {
    let mut response = (StatusCode::OK, body).into_response();
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        content_type
            .parse()
            .expect("static report content type is valid"),
    );
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        "no-store".parse().expect("static cache header is valid"),
    );
    if html {
        response.headers_mut().insert(
            "content-security-policy",
            "default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'"
                .parse()
                .expect("static CSP is valid"),
        );
    }
    response
}
