use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Semaphore;
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    models::{AssetRevision, NatsEventOutboxRecord, SuiteRunRecord},
    variables::{resolve_variable_definitions, ResolutionContext, VariableResolver},
    AppState,
};

pub struct Orchestrator {
    state: AppState,
    semaphore: Arc<Semaphore>,
    active_runs: Arc<Mutex<HashSet<String>>>,
}

struct ActiveRunGuard {
    active_runs: Arc<Mutex<HashSet<String>>>,
    run_id: String,
}

impl Drop for ActiveRunGuard {
    fn drop(&mut self) {
        if let Ok(mut active_runs) = self.active_runs.lock() {
            active_runs.remove(&self.run_id);
        }
    }
}

struct ResourceLockHeartbeat(tokio::task::JoinHandle<()>);

impl Drop for ResourceLockHeartbeat {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn collect_secret_references(value: &Value, references: &mut HashSet<String>) {
    match value {
        Value::Object(object) => {
            for (key, nested) in object {
                let normalized_key = key.to_ascii_lowercase();
                if (normalized_key.ends_with("_secret") || normalized_key == "secret_ref")
                    && nested.as_str().is_some_and(|value| !value.is_empty())
                {
                    references.insert(nested.as_str().unwrap_or_default().to_string());
                } else {
                    collect_secret_references(nested, references);
                }
            }
        }
        Value::Array(items) => {
            for nested in items {
                collect_secret_references(nested, references);
            }
        }
        _ => {}
    }
}

fn replace_secret_references(value: &mut Value, replacements: &HashMap<String, String>) {
    match value {
        Value::Object(object) => {
            for (key, nested) in object {
                let normalized_key = key.to_ascii_lowercase();
                if normalized_key.ends_with("_secret") || normalized_key == "secret_ref" {
                    if let Some(reference) = nested.as_str() {
                        if let Some(name) = replacements.get(reference) {
                            *nested = json!(name);
                        }
                    }
                } else {
                    replace_secret_references(nested, replacements);
                }
            }
        }
        Value::Array(items) => {
            for nested in items {
                replace_secret_references(nested, replacements);
            }
        }
        _ => {}
    }
}

impl Orchestrator {
    pub fn new(state: AppState) -> Self {
        let max_cases = state.config.max_active_cases;
        Self {
            state,
            semaphore: Arc::new(Semaphore::new(max_cases)),
            active_runs: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Mark executions left active by a control-plane restart as interrupted.
    /// These runs are not resumed because the external side effects of a node
    /// cannot be safely replayed without an idempotency contract.
    pub async fn recover_interrupted_runs(state: &AppState) -> Result<(), anyhow::Error> {
        let active_runs = sqlx::query_as::<_, (String, String, Option<String>)>(
            "SELECT id, workspace_id, initiating_user_id FROM suite_runs WHERE status = 'RUNNING' ORDER BY created_at",
        )
        .fetch_all(&state.db)
        .await?;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()?;
        for (run_id, workspace_id, actor_id) in active_runs {
            if let (Some(url), Some(token)) = (
                state.config.worker_manager_url.as_deref(),
                state.config.worker_manager_token.as_deref(),
            ) {
                match client
                    .post(format!("{}/v1/cancel", url))
                    .bearer_auth(token)
                    .json(&json!({ "run_id": run_id }))
                    .send()
                    .await
                {
                    Ok(response) if response.status().is_success() => {}
                    Ok(response) => warn!(
                        "Worker manager did not confirm cleanup for interrupted run {} (HTTP {})",
                        run_id,
                        response.status()
                    ),
                    Err(error) => warn!(
                        "Could not contact worker manager while recovering run {}: {}",
                        run_id, error
                    ),
                }
            }

            let now = Utc::now().to_rfc3339();
            let mut tx = state.db.begin().await?;
            let changed = sqlx::query(
                "UPDATE suite_runs SET status = 'INTERRUPTED', finished_at = ? WHERE id = ? AND status = 'RUNNING'",
            )
            .bind(&now)
            .bind(&run_id)
            .execute(&mut *tx)
            .await?;
            if changed.rows_affected() == 0 {
                tx.rollback().await?;
                continue;
            }
            sqlx::query(
                "UPDATE case_runs SET status = 'ERROR', finished_at = ? WHERE suite_run_id = ? AND status = 'RUNNING'",
            )
            .bind(&now)
            .bind(&run_id)
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "UPDATE step_runs SET status = 'INTERRUPTED', finished_at = ? WHERE case_run_id IN (SELECT id FROM case_runs WHERE suite_run_id = ?) AND status = 'RUNNING'",
            )
            .bind(&now)
            .bind(&run_id)
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "UPDATE resource_locks SET status = 'UNCERTAIN' WHERE run_id = ? AND status = 'HELD'",
            )
            .bind(&run_id)
            .execute(&mut *tx)
            .await?;
            let uncertain_keys = sqlx::query_scalar::<_, String>(
                "SELECT resource_key FROM resource_locks WHERE run_id = ? AND status = 'UNCERTAIN'",
            )
            .bind(&run_id)
            .fetch_all(&mut *tx)
            .await?;
            if !uncertain_keys.is_empty() {
                sqlx::query(
                    "INSERT INTO audit_events (id, actor_id, action, target_type, target_id, changes_json, created_at)
                     VALUES (?, ?, 'resource_lock.uncertain', 'suite_run', ?, ?, ?)",
                )
                .bind(Uuid::new_v4().to_string())
                .bind(actor_id.as_deref())
                .bind(&run_id)
                .bind(json!({ "resource_keys": uncertain_keys, "reason": "control_plane_restart" }).to_string())
                .bind(&now)
                .execute(&mut *tx)
                .await?;
            }

            let sequence: i64 = sqlx::query_scalar(
                "SELECT COALESCE(MAX(sequence), 0) + 1 FROM run_events WHERE suite_run_id = ?",
            )
            .bind(&run_id)
            .fetch_one(&mut *tx)
            .await?;
            let payload = json!({
                "schema_version": 1,
                "run_id": run_id,
                "status": "INTERRUPTED",
                "sequence": sequence,
                "event_type": "run.finished",
                "occurred_at": now
            });
            sqlx::query(
                "INSERT INTO run_events (id, suite_run_id, sequence, event_type, payload_json, occurred_at) VALUES (?, ?, ?, 'run.finished', ?, ?)",
            )
            .bind(Uuid::new_v4().to_string())
            .bind(&run_id)
            .bind(sequence)
            .bind(payload.to_string())
            .bind(&now)
            .execute(&mut *tx)
            .await?;
            let subject = format!("automation.v1.{}.runs.{}.stats", workspace_id, run_id);
            sqlx::query(
                "INSERT INTO nats_event_outbox (id, suite_run_id, sequence, subject, payload_json, status, created_at) VALUES (?, ?, ?, ?, ?, 'PENDING', ?)",
            )
            .bind(Uuid::new_v4().to_string())
            .bind(&run_id)
            .bind(sequence)
            .bind(subject)
            .bind(payload.to_string())
            .bind(&now)
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "UPDATE run_progress_snapshots SET status = 'INTERRUPTED', last_sequence = ?, updated_at = ? WHERE suite_run_id = ?",
            )
            .bind(sequence)
            .bind(&now)
            .bind(&run_id)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            warn!(
                "Marked run {} interrupted after control-plane restart",
                run_id
            );
        }
        Ok(())
    }

    /// Background loop polling for queued runs
    pub async fn run_loop(self: Arc<Self>) {
        info!(
            "Orchestrator scheduler loop started (Max concurrency: {})",
            self.state.config.max_active_cases
        );

        let mut interval = tokio::time::interval(Duration::from_millis(500));
        loop {
            interval.tick().await;

            let queued_runs = sqlx::query_as::<_, SuiteRunRecord>(
                "SELECT * FROM suite_runs WHERE status = 'QUEUED' ORDER BY created_at ASC LIMIT 200",
            )
            .fetch_all(&self.state.db)
            .await;
            let Ok(queued_runs) = queued_runs else {
                continue;
            };
            for run in queued_runs {
                let run_id = run.id.clone();
                let should_schedule = if let Ok(mut active) = self.active_runs.lock() {
                    active.len() < 64 && active.insert(run_id.clone())
                } else {
                    false
                };
                if should_schedule {
                    let this = self.clone();
                    let active_runs = self.active_runs.clone();
                    tokio::spawn(async move {
                        let _guard = ActiveRunGuard {
                            active_runs,
                            run_id,
                        };
                        this.execute_suite_run(run).await;
                    });
                }
            }
        }
    }

    async fn try_claim_run(
        &self,
        run: &SuiteRunRecord,
        resource_keys: &[String],
        started_at: &str,
    ) -> Result<bool, sqlx::Error> {
        let mut tx = self.state.db.begin().await?;
        for resource_key in resource_keys {
            let existing = sqlx::query_as::<_, (String, String, Option<String>, String, i64)>(
                "SELECT locks.run_id, locks.status, holder.status, locks.owner,
                        CASE WHEN julianday(locks.lease_expires_at) <= julianday('now') THEN 1 ELSE 0 END
                 FROM resource_locks locks
                 LEFT JOIN suite_runs holder ON holder.id = locks.run_id
                 WHERE locks.resource_key = ?",
            )
            .bind(resource_key)
            .fetch_optional(&mut *tx)
            .await?;
            let Some((holder_run_id, status, holder_status, owner_id, lease_expired)) = existing
            else {
                continue;
            };
            if holder_run_id == run.id && status == "HELD" {
                continue;
            }
            if matches!(status.as_str(), "EXPIRED" | "UNCERTAIN") {
                tx.rollback().await?;
                return Ok(false);
            }
            if holder_status
                .as_deref()
                .is_none_or(|holder_status| !matches!(holder_status, "QUEUED" | "RUNNING"))
            {
                sqlx::query(
                    "INSERT INTO audit_events (id, actor_id, action, target_type, target_id, changes_json, created_at)
                     VALUES (?, ?, 'resource_lock.release_recovered', 'resource_lock', ?, ?, ?)",
                )
                .bind(Uuid::new_v4().to_string())
                .bind((owner_id != "system").then_some(owner_id.as_str()))
                .bind(resource_key)
                .bind(json!({ "run_id": holder_run_id, "reason": "holder_run_terminal" }).to_string())
                .bind(Utc::now().to_rfc3339())
                .execute(&mut *tx)
                .await?;
                sqlx::query("DELETE FROM resource_locks WHERE resource_key = ? AND run_id = ?")
                    .bind(resource_key)
                    .bind(holder_run_id)
                    .execute(&mut *tx)
                    .await?;
                continue;
            }
            if lease_expired == 1 && status == "HELD" {
                sqlx::query(
                    "UPDATE resource_locks SET status = 'UNCERTAIN' WHERE resource_key = ? AND run_id = ? AND status = 'HELD'",
                )
                .bind(resource_key)
                .bind(&holder_run_id)
                .execute(&mut *tx)
                .await?;
                sqlx::query(
                    "INSERT INTO audit_events (id, actor_id, action, target_type, target_id, changes_json, created_at)
                     VALUES (?, ?, 'resource_lock.uncertain', 'resource_lock', ?, ?, ?)",
                )
                .bind(Uuid::new_v4().to_string())
                .bind((owner_id != "system").then_some(owner_id.as_str()))
                .bind(resource_key)
                .bind(json!({ "run_id": holder_run_id, "reason": "lease_expired_while_run_active" }).to_string())
                .bind(Utc::now().to_rfc3339())
                .execute(&mut *tx)
                .await?;
            }
            tx.commit().await?;
            return Ok(false);
        }

        for resource_key in resource_keys {
            let inserted = sqlx::query(
                "INSERT INTO resource_locks (resource_key, run_id, owner, lease_expires_at, status)
                 VALUES (?, ?, ?, datetime('now', '+45 seconds'), 'HELD')
                 ON CONFLICT(resource_key) DO UPDATE
                 SET lease_expires_at = excluded.lease_expires_at
                 WHERE resource_locks.run_id = excluded.run_id AND resource_locks.status = 'HELD'",
            )
            .bind(resource_key)
            .bind(&run.id)
            .bind(run.initiating_user_id.as_deref().unwrap_or("system"))
            .execute(&mut *tx)
            .await?;
            if inserted.rows_affected() != 1 {
                tx.rollback().await?;
                return Ok(false);
            }
        }
        let claimed = sqlx::query(
            "UPDATE suite_runs SET status = 'RUNNING', started_at = ? WHERE id = ? AND status = 'QUEUED'",
        )
        .bind(started_at)
        .bind(&run.id)
        .execute(&mut *tx)
        .await?;
        if claimed.rows_affected() != 1 {
            tx.rollback().await?;
            return Ok(false);
        }
        tx.commit().await?;
        Ok(true)
    }

    async fn update_resource_wait_snapshot(
        &self,
        run_id: &str,
        resource_names: &[String],
        timeout_seconds: i64,
    ) -> DateTime<Utc> {
        let stored = sqlx::query_scalar::<_, String>(
            "SELECT stats_json FROM run_progress_snapshots WHERE suite_run_id = ?",
        )
        .bind(run_id)
        .fetch_optional(&self.state.db)
        .await
        .ok()
        .flatten();
        let mut stats = stored
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}));
        let waiting_since = stats
            .get("lock_wait_started_at")
            .and_then(Value::as_str)
            .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
            .map(|timestamp| timestamp.with_timezone(&Utc))
            .unwrap_or_else(Utc::now);
        if let Some(object) = stats.as_object_mut() {
            object.insert("waiting_for_resources".to_string(), json!(resource_names));
            object.insert(
                "lock_wait_timeout_seconds".to_string(),
                json!(timeout_seconds),
            );
            object.insert(
                "lock_wait_started_at".to_string(),
                json!(waiting_since.to_rfc3339()),
            );
        }
        let now = Utc::now().to_rfc3339();
        let _ = sqlx::query(
            "UPDATE run_progress_snapshots SET stats_json = ?, updated_at = ? WHERE suite_run_id = ? AND status = 'QUEUED'",
        )
        .bind(stats.to_string())
        .bind(now)
        .bind(run_id)
        .execute(&self.state.db)
        .await;
        waiting_since
    }

    fn start_resource_lock_heartbeat(&self, run_id: String) -> ResourceLockHeartbeat {
        let db = self.state.db.clone();
        ResourceLockHeartbeat(tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(10));
            loop {
                interval.tick().await;
                match sqlx::query(
                    "UPDATE resource_locks SET lease_expires_at = datetime('now', '+45 seconds')
                     WHERE run_id = ? AND status = 'HELD'",
                )
                .bind(&run_id)
                .execute(&db)
                .await
                {
                    Ok(result) if result.rows_affected() == 0 => break,
                    Ok(_) => {}
                    Err(error) => warn!(
                        "Could not refresh resource-lock lease for {}: {}",
                        run_id, error
                    ),
                }
            }
        }))
    }

    async fn release_resource_locks(&self, run_id: &str) {
        let mut tx = match self.state.db.begin().await {
            Ok(tx) => tx,
            Err(error) => {
                warn!(
                    "Could not begin resource-lock release for {}: {}",
                    run_id, error
                );
                return;
            }
        };
        let locks = match sqlx::query_as::<_, (String, String)>(
            "SELECT resource_key, owner FROM resource_locks WHERE run_id = ? AND status = 'HELD'",
        )
        .bind(run_id)
        .fetch_all(&mut *tx)
        .await
        {
            Ok(locks) => locks,
            Err(error) => {
                let _ = tx.rollback().await;
                warn!("Could not read resource locks for {}: {}", run_id, error);
                return;
            }
        };
        let now = Utc::now().to_rfc3339();
        for (resource_key, owner_id) in &locks {
            let audit = sqlx::query(
                "INSERT INTO audit_events (id, actor_id, action, target_type, target_id, changes_json, created_at)
                 VALUES (?, ?, 'resource_lock.release', 'resource_lock', ?, ?, ?)",
            )
            .bind(Uuid::new_v4().to_string())
            .bind((owner_id != "system").then_some(owner_id.as_str()))
            .bind(resource_key)
            .bind(json!({ "run_id": run_id, "reason": "run_finished" }).to_string())
            .bind(&now)
            .execute(&mut *tx)
            .await;
            if let Err(error) = audit {
                let _ = tx.rollback().await;
                warn!(
                    "Could not audit resource-lock release for {}: {}",
                    run_id, error
                );
                return;
            }
        }
        if let Err(error) =
            sqlx::query("DELETE FROM resource_locks WHERE run_id = ? AND status = 'HELD'")
                .bind(run_id)
                .execute(&mut *tx)
                .await
        {
            let _ = tx.rollback().await;
            warn!("Could not release resource locks for {}: {}", run_id, error);
            return;
        }
        if let Err(error) = tx.commit().await {
            warn!(
                "Could not commit resource-lock release for {}: {}",
                run_id, error
            );
        }
    }

    /// Background loop draining NATS outbox
    pub async fn outbox_drain_loop(state: AppState) {
        info!("NATS transactional outbox publisher loop started");
        let mut interval = tokio::time::interval(Duration::from_millis(250));

        loop {
            interval.tick().await;

            let pending = sqlx::query_as::<_, NatsEventOutboxRecord>(
                "SELECT * FROM nats_event_outbox WHERE status = 'PENDING' AND julianday(next_attempt_at) <= julianday('now') ORDER BY sequence ASC LIMIT 50"
            )
            .fetch_all(&state.db)
            .await
            .unwrap_or_default();

            for item in pending {
                let published = if let Some(ref nats_client) = state.nats {
                    match nats_client
                        .publish(item.subject.clone(), item.payload_json.clone().into())
                        .await
                    {
                        Ok(_) => true,
                        Err(e) => {
                            warn!("NATS publish failed (retaining in outbox): {}", e);
                            false
                        }
                    }
                } else {
                    false
                };

                if published {
                    let _ = sqlx::query(
                        "UPDATE nats_event_outbox SET status = 'PUBLISHED' WHERE id = ?",
                    )
                    .bind(&item.id)
                    .execute(&state.db)
                    .await;
                } else {
                    let _ = sqlx::query(
                        "UPDATE nats_event_outbox SET attempts = attempts + 1, next_attempt_at = datetime('now', '+2 seconds') WHERE id = ?",
                    )
                    .bind(&item.id)
                    .execute(&state.db)
                    .await;
                }
            }
        }
    }

    /// Execute a full suite run
    async fn execute_suite_run(&self, run: SuiteRunRecord) {
        let run_id = run.id.clone();
        let rev = sqlx::query_as::<_, AssetRevision>("SELECT * FROM asset_revisions WHERE id = ?")
            .bind(&run.suite_revision_id)
            .fetch_optional(&self.state.db)
            .await;
        let suite_def: Value = match rev {
            Ok(Some(revision)) => match serde_json::from_str(&revision.definition_json) {
                Ok(definition) => definition,
                Err(_) => {
                    self.fail_run(&run_id, "Suite revision contains invalid definition data")
                        .await;
                    return;
                }
            },
            _ => {
                self.fail_run(&run_id, "Suite definition revision could not be loaded")
                    .await;
                return;
            }
        };
        let (resource_names, lock_timeout_seconds) =
            match crate::resource_locks::validate_suite_resource_locks(&suite_def) {
                Ok(settings) => settings,
                Err(message) => {
                    self.fail_run(&run_id, &message).await;
                    return;
                }
            };
        let suite_setup_nodes = suite_def
            .get("setup_nodes")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let suite_cleanup_nodes = suite_def
            .get("cleanup_nodes")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let resource_keys = resource_names
            .iter()
            .map(|name| format!("{}|{}", run.workspace_id, name.to_ascii_lowercase()))
            .collect::<Vec<_>>();
        let mut wait_started = None;
        let mut waiting_event_sent = false;
        let now = loop {
            if !self.run_is_queued(&run_id).await {
                return;
            }
            match self
                .try_claim_run(&run, &resource_keys, &Utc::now().to_rfc3339())
                .await
            {
                Ok(true) => break Utc::now().to_rfc3339(),
                Ok(false) => {}
                Err(error) => {
                    warn!(
                        "Could not claim run {} or its resource locks: {}",
                        run_id, error
                    );
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    continue;
                }
            }
            let wait_started_at = match wait_started {
                Some(timestamp) => timestamp,
                None => {
                    let timestamp = self
                        .update_resource_wait_snapshot(
                            &run_id,
                            &resource_names,
                            lock_timeout_seconds,
                        )
                        .await;
                    wait_started = Some(timestamp);
                    timestamp
                }
            };
            let lock_deadline = wait_started_at + ChronoDuration::seconds(lock_timeout_seconds);
            if Utc::now() >= lock_deadline {
                self.fail_run(
                    &run_id,
                    &format!(
                        "Timed out after {} seconds waiting for resource locks: {}",
                        lock_timeout_seconds,
                        resource_names.join(", ")
                    ),
                )
                .await;
                return;
            }
            if !waiting_event_sent {
                self.emit_event(
                    &run_id,
                    0,
                    "run.waiting_for_resource",
                    json!({
                        "run_id": run_id,
                        "resources": resource_names,
                        "timeout_seconds": lock_timeout_seconds
                    }),
                )
                .await;
                waiting_event_sent = true;
            }
            self.update_resource_wait_snapshot(&run_id, &resource_names, lock_timeout_seconds)
                .await;
            tokio::time::sleep(Duration::from_secs(1)).await;
        };
        info!("Starting execution for SuiteRun {}", run_id);
        let _resource_heartbeat = if resource_keys.is_empty() {
            None
        } else {
            Some(self.start_resource_lock_heartbeat(run_id.clone()))
        };
        self.emit_event(
            &run_id,
            2,
            "run.started",
            json!({ "run_id": run_id, "status": "RUNNING" }),
        )
        .await;

        let mut cases = suite_def
            .get("cases")
            .and_then(|c| c.as_array())
            .cloned()
            .unwrap_or_default();
        if run.source_run_id.is_some() {
            let failed_revisions = serde_json::from_str::<Value>(&run.run_manifest_json)
                .ok()
                .and_then(|manifest| manifest.get("rerun_case_revision_ids").cloned())
                .and_then(|value| value.as_array().cloned())
                .unwrap_or_default()
                .into_iter()
                .filter_map(|value| value.as_str().map(ToOwned::to_owned))
                .collect::<HashSet<_>>();
            cases.retain(|case_ref| {
                case_ref
                    .get("revision_id")
                    .and_then(Value::as_str)
                    .is_some_and(|revision_id| failed_revisions.contains(revision_id))
            });
        }
        let rerun_iterations = serde_json::from_str::<Value>(&run.run_manifest_json)
            .ok()
            .and_then(|manifest| manifest.get("rerun_failed_iterations").cloned())
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|entry| {
                Some((
                    entry.get("case_revision_id")?.as_str()?.to_string(),
                    entry.get("iteration_index")?.as_u64()? as usize,
                ))
            })
            .fold(
                HashMap::<String, HashSet<usize>>::new(),
                |mut selected, (revision, index)| {
                    selected.entry(revision).or_default().insert(index);
                    selected
                },
            );
        let mut case_plan = Vec::<(String, String, Value, Vec<(usize, Value)>, String)>::new();
        let mut dataset_manifest = Vec::new();
        for case_ref in cases {
            let case_id = case_ref
                .get("case_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let pinned_revision_id = case_ref.get("revision_id").and_then(Value::as_str);
            let Some((case_revision_id, case_def, revision_checksum)) = self
                .load_case_definition(&case_id, pinned_revision_id)
                .await
            else {
                self.fail_run(
                    &run_id,
                    "A suite case is missing its pinned published revision",
                )
                .await;
                return;
            };
            let rows = match case_dataset_rows(&case_def) {
                Ok(rows) => rows,
                Err(message) => {
                    self.fail_run(&run_id, &message).await;
                    return;
                }
            };
            let mut indexed_rows = rows.into_iter().enumerate().collect::<Vec<_>>();
            if run.source_run_id.is_some() && !rerun_iterations.is_empty() {
                if let Some(indices) = rerun_iterations.get(&case_revision_id) {
                    indexed_rows.retain(|(index, _)| indices.contains(index));
                } else {
                    indexed_rows.clear();
                }
            }
            let dataset_definition = case_def
                .get("data_set")
                .or_else(|| case_def.get("dataset"))
                .cloned()
                .unwrap_or_else(|| json!({}));
            let dataset_checksum = serde_json::to_vec(&dataset_definition)
                .map(|bytes| crate::crypto::compute_sha256(&bytes))
                .unwrap_or_default();
            dataset_manifest.push(json!({
                "case_revision_id": case_revision_id,
                "case_revision_checksum": revision_checksum,
                "dataset_checksum": dataset_checksum,
                "row_count": indexed_rows.len()
            }));
            case_plan.push((
                case_id,
                case_revision_id,
                case_def,
                indexed_rows,
                revision_checksum,
            ));
        }
        let total_cases: usize = case_plan.iter().map(|entry| entry.3.len()).sum();
        let total_steps: usize = case_plan
            .iter()
            .map(|entry| {
                entry.3.len()
                    * entry
                        .2
                        .get("nodes")
                        .and_then(Value::as_array)
                        .map_or(0, Vec::len)
            })
            .sum::<usize>()
            + suite_setup_nodes.len()
            + suite_cleanup_nodes.len();
        if total_cases == 0 || total_cases > 500 {
            self.fail_run(
                &run_id,
                "The suite must expand to between 1 and 500 bounded case iterations",
            )
            .await;
            return;
        }
        if let Ok(mut manifest) = serde_json::from_str::<Value>(&run.run_manifest_json) {
            manifest["datasets"] = json!(dataset_manifest);
            manifest["planned_case_iterations"] = json!(total_cases);
            manifest["planned_node_invocations"] = json!(total_steps);
            manifest["suite_setup_nodes"] = json!(suite_setup_nodes.len());
            manifest["suite_cleanup_nodes"] = json!(suite_cleanup_nodes.len());
            let _ = sqlx::query("UPDATE suite_runs SET run_manifest_json = ? WHERE id = ?")
                .bind(manifest.to_string())
                .bind(&run_id)
                .execute(&self.state.db)
                .await;
        }
        self.emit_event(
            &run_id,
            0,
            "run.planned",
            json!({ "planned_case_iterations": total_cases, "planned_node_invocations": total_steps, "datasets": dataset_manifest }),
        )
        .await;

        let env_definitions = sqlx::query_scalar::<_, String>(
            "SELECT variables_json FROM environments WHERE id = ? AND workspace_id = ?",
        )
        .bind(&run.environment_id)
        .bind(&run.workspace_id)
        .fetch_optional(&self.state.db)
        .await
        .ok()
        .flatten()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or_else(|| json!({}));
        let mut run_vars = json_object_map(&run.inputs_json);
        run_vars.extend(json_object_map(&run.variable_overrides_json));
        let mut env_context = ResolutionContext::default();
        env_context.run_id = run_id.clone();
        env_context.suite_id = suite_def
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or(&run.suite_revision_id)
            .to_string();
        env_context.started_at_utc = run.started_at.clone().unwrap_or_else(|| now.clone());
        env_context.seed = run.random_seed as u64;
        let env_vars =
            match resolve_variable_definitions(Some(&env_definitions), &env_context, "env") {
                Ok(values) => values,
                Err(message) => {
                    self.fail_run(
                        &run_id,
                        &format!("Environment variables could not be resolved: {message}"),
                    )
                    .await;
                    return;
                }
            };
        let mut suite_context = ResolutionContext::default();
        suite_context.run_id = run_id.clone();
        suite_context.suite_id = suite_def
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or(&run.suite_revision_id)
            .to_string();
        suite_context.started_at_utc = run.started_at.clone().unwrap_or_else(|| now.clone());
        suite_context.seed = run.random_seed as u64;
        suite_context.env_vars = env_vars.clone();
        suite_context.run_vars = run_vars.clone();
        let suite_vars =
            match resolve_variable_definitions(suite_def.get("variables"), &suite_context, "suite")
            {
                Ok(values) => values,
                Err(message) => {
                    self.fail_run(
                        &run_id,
                        &format!("Suite variables could not be resolved: {message}"),
                    )
                    .await;
                    return;
                }
            };
        suite_context.suite_vars = suite_vars.clone();

        let mut seq = 3i64;
        let suite_setup_status = self
            .execute_suite_hook_nodes(
                &run_id,
                &run.suite_revision_id,
                "suite_setup",
                &suite_setup_nodes,
                &mut suite_context,
                &mut seq,
                false,
            )
            .await;
        let mut suite_status = suite_setup_status.clone();
        let mut passed_cases = 0;
        let mut failed_cases = 0;
        let mut errored_cases = 0;
        let mut canceled_cases = 0;
        let mut skipped_cases = 0;
        let mut ordinal = 0usize;
        'case_plan: for (case_id, case_revision_id, case_def, rows, _checksum) in &case_plan {
            for (iteration_index, row) in rows {
                if suite_status != "PASSED" {
                    break 'case_plan;
                }
                if self.run_is_canceled(&run_id).await {
                    suite_status = "CANCELED".to_string();
                    break 'case_plan;
                }
                let permit = match self.semaphore.clone().acquire_owned().await {
                    Ok(p) => p,
                    Err(_) => {
                        suite_status = "ERROR".to_string();
                        break 'case_plan;
                    }
                };
                let case_run_id = Uuid::new_v4().to_string();
                let iteration_id = Uuid::new_v4().to_string();
                let case_started = Utc::now().to_rfc3339();
                let report_inputs = json!({ "iteration_id": iteration_id, "row": row });

                let _ = sqlx::query(
                    "INSERT INTO case_runs (id, suite_run_id, case_revision_id, ordinal, iteration_index, status, inputs_json, started_at)
                     VALUES (?, ?, ?, ?, ?, 'RUNNING', ?, ?)"
                )
                .bind(&case_run_id)
                .bind(&run_id)
                .bind(case_revision_id)
                .bind(ordinal as i64)
                .bind(*iteration_index as i64)
                .bind(report_inputs.to_string())
                .bind(&case_started)
                .execute(&self.state.db)
                .await;

                let mut context = ResolutionContext::default();
                context.run_id = run_id.clone();
                context.suite_id = suite_def
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or(&run.suite_revision_id)
                    .to_string();
                context.case_id = case_id.clone();
                context.iteration_id = iteration_id;
                context.started_at_utc = run.started_at.clone().unwrap_or_else(|| now.clone());
                let case_seed = case_revision_id
                    .bytes()
                    .fold(run.random_seed as u64, |seed, byte| {
                        seed.wrapping_mul(31).wrapping_add(byte as u64)
                    });
                context.seed = case_seed.wrapping_add(*iteration_index as u64);
                context.env_vars = env_vars.clone();
                context.run_vars = run_vars.clone();
                context.suite_vars = suite_vars.clone();
                context.step_outputs = suite_context.step_outputs.clone();
                context.iteration_vars = row
                    .as_object()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .collect();
                let variable_error =
                    match resolve_variable_definitions(case_def.get("variables"), &context, "case")
                    {
                        Ok(values) => {
                            context.case_vars = values;
                            None
                        }
                        Err(message) => Some(message),
                    };

                seq += 1;
                self.emit_event(
                    &run_id,
                    seq,
                    "case.started",
                    json!({ "case_run_id": case_run_id, "ordinal": ordinal, "iteration_index": iteration_index, "status": "RUNNING" }),
                )
                .await;
                let case_status = if let Some(message) = &variable_error {
                    self.emit_event(
                        &run_id,
                        seq,
                        "case.variable_error",
                        json!({ "case_run_id": case_run_id, "message": message }),
                    )
                    .await;
                    "ERROR".to_string()
                } else {
                    self.execute_case_nodes(
                        &run_id,
                        &case_run_id,
                        &case_def,
                        &mut context,
                        &mut seq,
                    )
                    .await
                };
                let case_finished = Utc::now().to_rfc3339();
                let canceled = self.run_is_canceled(&run_id).await || case_status == "CANCELED";
                let case_status_str = if canceled {
                    "CANCELED"
                } else {
                    case_status.as_str()
                };

                match case_status_str {
                    "PASSED" => passed_cases += 1,
                    "FAILED" => {
                        failed_cases += 1;
                        if suite_status == "PASSED" {
                            suite_status = "FAILED".to_string();
                        }
                    }
                    "ERROR" => {
                        errored_cases += 1;
                        if suite_status != "CANCELED" {
                            suite_status = "ERROR".to_string();
                        }
                    }
                    "CANCELED" => {
                        canceled_cases += 1;
                        suite_status = "CANCELED".to_string();
                    }
                    _ => {}
                }
                let _ =
                    sqlx::query("UPDATE case_runs SET status = ?, finished_at = ? WHERE id = ?")
                        .bind(case_status_str)
                        .bind(&case_finished)
                        .bind(&case_run_id)
                        .execute(&self.state.db)
                        .await;
                seq += 1;
                self.emit_event(
                    &run_id,
                    seq,
                    "case.finished",
                    json!({
                        "case_run_id": case_run_id,
                        "ordinal": ordinal,
                        "iteration_index": iteration_index,
                        "status": case_status_str,
                        "finished_at": case_finished,
                        "error": variable_error
                    }),
                )
                .await;
                drop(permit);
                ordinal += 1;
                if canceled {
                    break 'case_plan;
                }
            }
        }

        if ordinal < total_cases {
            let reason = if suite_setup_status != "PASSED" {
                "Suite setup did not pass"
            } else if suite_status == "CANCELED" {
                "Run was canceled before this case iteration began"
            } else {
                "Run stopped before this case iteration began"
            };
            skipped_cases += self
                .record_skipped_case_plan(&run_id, &case_plan, ordinal, reason, &mut seq)
                .await;
        }

        let suite_cleanup_status = self
            .execute_suite_hook_nodes(
                &run_id,
                &run.suite_revision_id,
                "suite_cleanup",
                &suite_cleanup_nodes,
                &mut suite_context,
                &mut seq,
                true,
            )
            .await;
        if suite_status == "PASSED" && suite_cleanup_status != "PASSED" {
            suite_status = suite_cleanup_status;
        }

        // Finalize suite run
        let finish_time = Utc::now().to_rfc3339();
        let _ = sqlx::query("UPDATE suite_runs SET status = CASE WHEN status = 'CANCELED' THEN 'CANCELED' ELSE ? END, finished_at = ? WHERE id = ?")
            .bind(&suite_status)
            .bind(&finish_time)
            .bind(&run_id)
            .execute(&self.state.db)
            .await;
        self.release_resource_locks(&run_id).await;

        let terminal_status =
            sqlx::query_scalar::<_, String>("SELECT status FROM suite_runs WHERE id = ?")
                .bind(&run_id)
                .fetch_optional(&self.state.db)
                .await
                .ok()
                .flatten()
                .unwrap_or_else(|| suite_status.to_string());

        // Update final progress snapshot
        let final_progress = json!({
            "mode": "determinate",
            "percent": 100,
            "terminal_nodes": total_steps,
            "planned_nodes": total_steps
        });
        let final_stats = json!({
            "cases_total": total_cases,
            "cases_passed": passed_cases,
            "cases_failed": failed_cases,
            "cases_errored": errored_cases,
            "cases_canceled": canceled_cases,
            "cases_skipped": skipped_cases,
            "cases_completed": passed_cases + failed_cases + errored_cases + canceled_cases + skipped_cases
        });

        seq += 1;
        let _ = sqlx::query(
            "UPDATE run_progress_snapshots SET status = ?, progress_json = ?, stats_json = ?, last_sequence = ?, updated_at = ? WHERE suite_run_id = ?"
        )
        .bind(&terminal_status)
        .bind(final_progress.to_string())
        .bind(final_stats.to_string())
        .bind(seq)
        .bind(&finish_time)
        .bind(&run_id)
        .execute(&self.state.db)
        .await;

        self.emit_event(
            &run_id,
            seq,
            "run.finished",
            json!({ "run_id": run_id, "status": terminal_status, "finished_at": finish_time }),
        )
        .await;

        info!(
            "SuiteRun {} completed with status: {}",
            run_id, terminal_status
        );
    }

    async fn execute_suite_hook_nodes(
        &self,
        run_id: &str,
        suite_revision_id: &str,
        execution_scope: &str,
        nodes: &[Value],
        context: &mut ResolutionContext,
        seq: &mut i64,
        cleanup: bool,
    ) -> String {
        if nodes.is_empty() {
            return "PASSED".to_string();
        }
        let hook_run_id = Uuid::new_v4().to_string();
        let started_at = Utc::now().to_rfc3339();
        let (ordinal, iteration_index) = if execution_scope == "suite_setup" {
            (-2, -1)
        } else {
            (i64::MAX, -2)
        };
        let inserted = sqlx::query(
            "INSERT INTO case_runs (id, suite_run_id, case_revision_id, execution_scope, ordinal, iteration_index, status, inputs_json, started_at)
             VALUES (?, ?, ?, ?, ?, ?, 'RUNNING', ?, ?)",
        )
        .bind(&hook_run_id)
        .bind(run_id)
        .bind(suite_revision_id)
        .bind(execution_scope)
        .bind(ordinal)
        .bind(iteration_index)
        .bind(json!({ "execution_scope": execution_scope }).to_string())
        .bind(&started_at)
        .execute(&self.state.db)
        .await;
        if !inserted.is_ok_and(|result| result.rows_affected() == 1) {
            return "ERROR".to_string();
        }

        *seq += 1;
        self.emit_event(
            run_id,
            *seq,
            "suite.hook.started",
            json!({ "suite_hook_run_id": hook_run_id, "scope": execution_scope, "status": "RUNNING" }),
        )
        .await;
        let indexed_nodes = nodes.iter().cloned().enumerate().collect::<Vec<_>>();
        let status = self
            .execute_node_group(run_id, &hook_run_id, &indexed_nodes, context, seq, cleanup)
            .await;
        let finished_at = Utc::now().to_rfc3339();
        let _ = sqlx::query("UPDATE case_runs SET status = ?, finished_at = ? WHERE id = ?")
            .bind(&status)
            .bind(&finished_at)
            .bind(&hook_run_id)
            .execute(&self.state.db)
            .await;
        *seq += 1;
        self.emit_event(
            run_id,
            *seq,
            "suite.hook.finished",
            json!({
                "suite_hook_run_id": hook_run_id,
                "scope": execution_scope,
                "status": status,
                "finished_at": finished_at
            }),
        )
        .await;
        status
    }

    async fn record_skipped_case_plan(
        &self,
        run_id: &str,
        case_plan: &[(String, String, Value, Vec<(usize, Value)>, String)],
        start_ordinal: usize,
        reason: &str,
        seq: &mut i64,
    ) -> usize {
        let mut ordinal = 0usize;
        let mut skipped = 0usize;
        for (case_id, case_revision_id, case_definition, rows, _) in case_plan {
            let nodes = case_definition
                .get("nodes")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for (iteration_index, row) in rows {
                if ordinal >= start_ordinal {
                    let case_run_id = Uuid::new_v4().to_string();
                    let iteration_id = Uuid::new_v4().to_string();
                    let finished_at = Utc::now().to_rfc3339();
                    let inputs = json!({
                        "iteration_id": iteration_id,
                        "row": row,
                        "skip_reason": reason
                    });
                    let inserted = sqlx::query(
                        "INSERT INTO case_runs (id, suite_run_id, case_revision_id, ordinal, iteration_index, status, inputs_json, finished_at)
                         VALUES (?, ?, ?, ?, ?, 'SKIPPED', ?, ?)",
                    )
                    .bind(&case_run_id)
                    .bind(run_id)
                    .bind(case_revision_id)
                    .bind(ordinal as i64)
                    .bind(*iteration_index as i64)
                    .bind(inputs.to_string())
                    .bind(&finished_at)
                    .execute(&self.state.db)
                    .await;
                    if inserted.is_err() {
                        warn!("Could not record skipped case iteration for run {}", run_id);
                        ordinal += 1;
                        continue;
                    }
                    let error =
                        json!({ "code": "CASE_NOT_STARTED", "message": reason }).to_string();
                    for (step_ordinal, node) in nodes.iter().enumerate() {
                        let step_run_id = Uuid::new_v4().to_string();
                        let node_name = node.get("name").and_then(Value::as_str).unwrap_or("Node");
                        let _ = sqlx::query(
                            "INSERT INTO step_runs (id, case_run_id, node_id, node_name, node_type, ordinal, attempt, status, error_json, finished_at)
                             VALUES (?, ?, ?, ?, ?, ?, 1, 'SKIPPED', ?, ?)",
                        )
                        .bind(&step_run_id)
                        .bind(&case_run_id)
                        .bind(node.get("id").and_then(Value::as_str).unwrap_or("unknown"))
                        .bind(node_name)
                        .bind(node.get("type").and_then(Value::as_str).unwrap_or("unknown"))
                        .bind(step_ordinal as i64)
                        .bind(&error)
                        .bind(&finished_at)
                        .execute(&self.state.db)
                        .await;
                        *seq += 1;
                        self.emit_event(
                            run_id,
                            *seq,
                            "step.finished",
                            json!({
                                "step_run_id": step_run_id,
                                "node_name": node_name,
                                "status": "SKIPPED",
                                "error": reason
                            }),
                        )
                        .await;
                    }
                    *seq += 1;
                    self.emit_event(
                        run_id,
                        *seq,
                        "case.finished",
                        json!({
                            "case_run_id": case_run_id,
                            "case_id": case_id,
                            "ordinal": ordinal,
                            "iteration_index": iteration_index,
                            "status": "SKIPPED",
                            "finished_at": finished_at,
                            "skip_reason": reason
                        }),
                    )
                    .await;
                    skipped += 1;
                }
                ordinal += 1;
            }
        }
        skipped
    }

    async fn execute_case_nodes(
        &self,
        run_id: &str,
        case_run_id: &str,
        case_def: &Value,
        context: &mut ResolutionContext,
        seq: &mut i64,
    ) -> String {
        let nodes = case_def
            .get("nodes")
            .and_then(|n| n.as_array())
            .cloned()
            .unwrap_or_default();
        let mut setup_nodes = Vec::new();
        let mut main_nodes = Vec::new();
        let mut cleanup_nodes = Vec::new();
        for (ordinal, node) in nodes.into_iter().enumerate() {
            let phase = node.get("phase").and_then(Value::as_str).unwrap_or("main");
            match phase {
                "setup" => setup_nodes.push((ordinal, node)),
                "cleanup" => cleanup_nodes.push((ordinal, node)),
                _ => main_nodes.push((ordinal, node)),
            }
        }

        let mut case_status = self
            .execute_node_group(run_id, case_run_id, &setup_nodes, context, seq, false)
            .await;
        if case_status == "PASSED" {
            case_status = self
                .execute_node_group(run_id, case_run_id, &main_nodes, context, seq, false)
                .await;
        } else {
            self.record_skipped_nodes(
                run_id,
                case_run_id,
                &main_nodes,
                "Case setup did not pass",
                seq,
            )
            .await;
        }

        // Cleanup nodes run after setup or main failure and after cancellation. The
        // worker manager only permits the cancellation bypass for this explicit phase.
        let cleanup_status = self
            .execute_node_group(run_id, case_run_id, &cleanup_nodes, context, seq, true)
            .await;
        if case_status == "PASSED" && cleanup_status != "PASSED" {
            case_status = cleanup_status;
        }

        case_status
    }

    async fn execute_node_group(
        &self,
        run_id: &str,
        case_run_id: &str,
        nodes: &[(usize, Value)],
        context: &mut ResolutionContext,
        seq: &mut i64,
        cleanup: bool,
    ) -> String {
        for (position, (ord, node)) in nodes.iter().enumerate() {
            if !cleanup && self.run_is_canceled(run_id).await {
                self.record_skipped_nodes(
                    run_id,
                    case_run_id,
                    &nodes[position..],
                    "Run was canceled before this step began",
                    seq,
                )
                .await;
                return "CANCELED".to_string();
            }
            let node_id = node.get("id").and_then(|i| i.as_str()).unwrap_or("unknown");
            let node_name = node.get("name").and_then(|n| n.as_str()).unwrap_or("Node");
            let node_type = node
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("api.request");
            let step_run_id = Uuid::new_v4().to_string();
            let start = Utc::now().to_rfc3339();

            let _ = sqlx::query(
                "INSERT INTO step_runs (id, case_run_id, node_id, node_name, node_type, ordinal, attempt, status, started_at)
                 VALUES (?, ?, ?, ?, ?, ?, 1, 'RUNNING', ?)"
            )
            .bind(&step_run_id)
            .bind(case_run_id)
            .bind(node_id)
            .bind(node_name)
            .bind(node_type)
            .bind(*ord as i64)
            .bind(&start)
            .execute(&self.state.db)
            .await;

            *seq += 1;
            self.emit_event(
                run_id,
                *seq,
                "step.started",
                json!({ "step_run_id": step_run_id, "node_name": node_name, "status": "RUNNING" }),
            )
            .await;

            // Execute node (with worker adapter dispatch)
            let (step_status, duration_ms, error_json, outputs_json, metrics_json) = self
                .dispatch_node_execution(run_id, case_run_id, &step_run_id, node, context, cleanup)
                .await;

            let finish = Utc::now().to_rfc3339();
            let _ = sqlx::query(
                "UPDATE step_runs SET status = ?, duration_ms = ?, error_json = ?, outputs_json = ?, metrics_json = ?, finished_at = ? WHERE id = ?"
            )
            .bind(&step_status)
            .bind(duration_ms)
            .bind(&error_json)
            .bind(&outputs_json)
            .bind(&metrics_json)
            .bind(&finish)
            .bind(&step_run_id)
            .execute(&self.state.db)
            .await;

            *seq += 1;
            self.emit_event(
                run_id,
                *seq,
                "step.finished",
                json!({
                    "step_run_id": step_run_id,
                    "status": step_status,
                    "duration_ms": duration_ms
                }),
            )
            .await;

            if step_status != "SUCCEEDED" {
                self.record_skipped_nodes(
                    run_id,
                    case_run_id,
                    &nodes[position + 1..],
                    "A prior step in this phase did not pass",
                    seq,
                )
                .await;
                let case_status = match step_status.as_str() {
                    "ASSERTION_FAILED" => "FAILED",
                    "CANCELED" => "CANCELED",
                    _ => "ERROR",
                };
                return case_status.to_string();
            }
        }

        "PASSED".to_string()
    }

    async fn record_skipped_nodes(
        &self,
        run_id: &str,
        case_run_id: &str,
        nodes: &[(usize, Value)],
        reason: &str,
        seq: &mut i64,
    ) {
        let finished_at = Utc::now().to_rfc3339();
        let error = json!({ "code": "STEP_NOT_STARTED", "message": reason }).to_string();
        for (ordinal, node) in nodes {
            let step_run_id = Uuid::new_v4().to_string();
            let _ = sqlx::query(
                "INSERT INTO step_runs (id, case_run_id, node_id, node_name, node_type, ordinal, attempt, status, error_json, finished_at)
                 VALUES (?, ?, ?, ?, ?, ?, 1, 'SKIPPED', ?, ?)",
            )
            .bind(&step_run_id)
            .bind(case_run_id)
            .bind(node.get("id").and_then(Value::as_str).unwrap_or("unknown"))
            .bind(node.get("name").and_then(Value::as_str).unwrap_or("Node"))
            .bind(node.get("type").and_then(Value::as_str).unwrap_or("unknown"))
            .bind(*ordinal as i64)
            .bind(&error)
            .bind(&finished_at)
            .execute(&self.state.db)
            .await;
            *seq += 1;
            self.emit_event(
                run_id,
                *seq,
                "step.finished",
                json!({
                    "step_run_id": step_run_id,
                    "node_name": node.get("name").and_then(Value::as_str).unwrap_or("Node"),
                    "status": "SKIPPED",
                    "error": reason
                }),
            )
            .await;
        }
    }

    async fn dispatch_node_execution(
        &self,
        run_id: &str,
        case_run_id: &str,
        step_run_id: &str,
        node: &Value,
        context: &mut ResolutionContext,
        cleanup: bool,
    ) -> (String, f64, Option<String>, Option<String>, Option<String>) {
        let node_type = node.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let timeout_secs = node
            .get("timeout_seconds")
            .and_then(|t| t.as_u64())
            .unwrap_or(30)
            .clamp(1, 3600);
        let error_result = |code: &str, message: &str| {
            (
                "ERROR".to_string(),
                0.0,
                Some(
                    json!({
                        "code": code,
                        "message": message,
                        "class": "INTERNAL",
                        "details": {}
                    })
                    .to_string(),
                ),
                None,
                None,
            )
        };
        let (Some(url), Some(token)) = (
            self.state.config.worker_manager_url.as_deref(),
            self.state.config.worker_manager_token.as_deref(),
        ) else {
            return error_result(
                "WORKER_UNAVAILABLE",
                "The isolated worker manager is not configured; this node was not executed",
            );
        };

        let (resolved_config, resolved_secrets) = match self
            .resolve_node_config_and_secrets(run_id, node, context)
            .await
        {
            Ok(resolved) => resolved,
            Err((code, message)) => return error_result(code, message),
        };

        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs + 20))
            .build()
        {
            Ok(client) => client,
            Err(_) => {
                return error_result(
                    "WORKER_CLIENT_ERROR",
                    "Could not initialize the worker client",
                )
            }
        };
        let mut resolver = VariableResolver::new(context);
        let resolved_inputs = match node
            .get("inputs")
            .filter(|value| value.is_object())
            .map(|value| resolver.resolve_json_value(value))
            .transpose()
        {
            Ok(Some(inputs)) => inputs,
            Ok(None) => json!({}),
            Err(_) => {
                return error_result(
                    "VARIABLE_RESOLUTION_FAILED",
                    "A configured input references a missing or invalid variable",
                )
            }
        };
        let envelope = json!({
            "schema_version": 1,
            "run_id": run_id,
            "case_id": case_run_id,
            "step_id": step_run_id,
            "attempt": 1,
            "deadline_utc": (Utc::now() + chrono::Duration::seconds(timeout_secs as i64)).to_rfc3339(),
            "node_type": node_type,
            "node_type_version": node.get("type_version").and_then(Value::as_u64).unwrap_or(1),
            "cleanup": cleanup,
            "config": resolved_config,
            "inputs": resolved_inputs,
            "secrets": resolved_secrets,
            "limits": {
                "max_output_bytes": 1_048_576,
                "max_log_bytes": 524_288,
                "timeout_seconds": timeout_secs
            }
        });
        let started = std::time::Instant::now();
        let response =
            match client
                .post(format!("{}/v1/invoke", url))
                .bearer_auth(token)
                .json(&envelope)
                .send()
                .await
            {
                Ok(response) => response,
                Err(_) => return error_result(
                    "WORKER_UNAVAILABLE",
                    "The isolated worker manager could not be reached; this node was not executed",
                ),
            };
        if !response.status().is_success() {
            return error_result(
                "WORKER_REJECTED",
                "The isolated worker manager rejected this node invocation",
            );
        }
        let body: Value = match response.json().await {
            Ok(value) => value,
            Err(_) => {
                return error_result(
                    "WORKER_PROTOCOL_ERROR",
                    "The worker manager returned invalid JSON",
                )
            }
        };
        let Some(result) = body.get("result") else {
            return error_result(
                "WORKER_PROTOCOL_ERROR",
                "The worker manager did not return a result frame",
            );
        };
        if result.get("frame_type").and_then(Value::as_str) != Some("result")
            || result.get("step_id").and_then(Value::as_str) != Some(step_run_id)
        {
            return error_result(
                "WORKER_PROTOCOL_ERROR",
                "The worker result did not match this step invocation",
            );
        }
        let status = result.get("status").and_then(Value::as_str).unwrap_or("");
        if !matches!(
            status,
            "SUCCEEDED" | "ASSERTION_FAILED" | "ERROR" | "TIMED_OUT" | "CANCELED"
        ) {
            return error_result(
                "WORKER_PROTOCOL_ERROR",
                "The worker returned an unsupported result status",
            );
        }
        let duration_ms = result
            .get("metrics")
            .and_then(|value| value.get("duration_ms"))
            .and_then(Value::as_f64)
            .unwrap_or_else(|| started.elapsed().as_secs_f64() * 1000.0);
        let error_json = result
            .get("error")
            .filter(|value| !value.is_null())
            .map(Value::to_string);
        let outputs_json = result
            .get("outputs")
            .filter(|value| value.is_object())
            .map(Value::to_string);
        if let Some(outputs) = result.get("outputs").and_then(Value::as_object) {
            let output_map: HashMap<String, Value> = outputs
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect();
            let node_id = node.get("id").and_then(Value::as_str).unwrap_or("unknown");
            let node_name = node.get("name").and_then(Value::as_str).unwrap_or(node_id);
            context
                .step_outputs
                .insert(node_id.to_string(), output_map.clone());
            context
                .step_outputs
                .insert(node_name.to_string(), output_map);
        }
        let metrics_json = result
            .get("metrics")
            .filter(|value| value.is_object())
            .map(Value::to_string);
        (
            status.to_string(),
            duration_ms,
            error_json,
            outputs_json,
            metrics_json,
        )
    }

    async fn resolve_node_config_and_secrets(
        &self,
        run_id: &str,
        node: &Value,
        context: &ResolutionContext,
    ) -> Result<(Value, Value), (&'static str, &'static str)> {
        let (workspace_id, actor_id) = sqlx::query_as::<_, (String, Option<String>)>(
            "SELECT workspace_id, initiating_user_id FROM suite_runs WHERE id = ?",
        )
        .bind(run_id)
        .fetch_optional(&self.state.db)
        .await
        .ok()
        .flatten()
        .ok_or(("RUN_NOT_FOUND", "The run workspace could not be loaded"))?;

        let mut node_config = node
            .get("config")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let connection_id = node_config
            .remove("connection_id")
            .and_then(|value| value.as_str().map(ToOwned::to_owned));
        if let Some(connection_id) = connection_id {
            let profile = sqlx::query_as::<_, (String, String, String)>(
                "SELECT connector_type, settings_json, secret_refs_json FROM connection_profiles WHERE id = ? AND workspace_id = ?",
            )
            .bind(connection_id)
            .bind(&workspace_id)
            .fetch_optional(&self.state.db)
            .await
            .ok()
            .flatten()
            .ok_or(("CONNECTION_NOT_FOUND", "The selected connection profile is unavailable"))?;
            let node_type = node.get("type").and_then(Value::as_str).unwrap_or("");
            let wait_target = node_config
                .get("target")
                .and_then(Value::as_str)
                .unwrap_or("api");
            let compatible = matches!(
                (profile.0.as_str(), node_type),
                ("mysql", "db.mysql")
                    | ("cassandra", "db.cassandra")
                    | ("mongodb", "db.mongodb")
                    | ("api", "api.request")
                    | ("http", "api.request")
                    | ("parquet", "data.tabular")
                    | ("delta", "data.tabular")
            ) || (node_type == "wait.until"
                && matches!(
                    (profile.0.as_str(), wait_target),
                    ("api" | "http", "api")
                        | ("mysql", "mysql")
                        | ("cassandra", "cassandra")
                        | ("mongodb", "mongodb")
                ));
            if !compatible {
                return Err((
                    "CONNECTION_TYPE_MISMATCH",
                    "The connection profile type does not match this node",
                ));
            }
            let mut settings: serde_json::Map<String, Value> = serde_json::from_str(&profile.1)
                .map_err(|_| {
                    (
                        "CONNECTION_CONFIG_INVALID",
                        "The connection profile settings are invalid",
                    )
                })?;
            let secret_refs: Value = serde_json::from_str(&profile.2).map_err(|_| {
                (
                    "CONNECTION_CONFIG_INVALID",
                    "The connection profile secret references are invalid",
                )
            })?;
            if let Some(refs) = secret_refs.as_object() {
                for (key, reference) in refs {
                    settings
                        .entry(key.clone())
                        .or_insert_with(|| reference.clone());
                }
            }
            for (key, value) in node_config {
                settings.insert(key, value);
            }
            node_config = settings;
        }

        // OpenAPI imports can provide their server URL directly on the node and
        // do not require a saved connection profile. Resolve the same base/path
        // combination for both imported drafts and profile-backed requests.
        if let (Some(base), Some(path)) = (
            node_config.get("base_url").and_then(Value::as_str),
            node_config.get("path").and_then(Value::as_str),
        ) {
            if node_config.get("url").is_none() {
                node_config.insert(
                    "url".to_string(),
                    json!(format!(
                        "{}{}{}",
                        base.trim_end_matches('/'),
                        if path.starts_with('/') { "" } else { "/" },
                        path
                    )),
                );
            }
        }

        let mut resolver = VariableResolver::new(context);
        let node_type = node.get("type").and_then(Value::as_str).unwrap_or("");
        if (matches!(node_type, "db.mysql" | "db.cassandra")
            || (node_type == "wait.until"
                && node_config.get("target").and_then(Value::as_str) == Some("mysql")))
            && node_config
                .get("query")
                .and_then(Value::as_str)
                .is_some_and(|query| query.contains("{{"))
        {
            return Err((
                "SQL_PARAMETER_REQUIRED",
                "SQL values must use bound parameters; variable interpolation in query text is disabled",
            ));
        }
        let resolved_node_config = resolver
            .resolve_json_value(&Value::Object(node_config))
            .map_err(|_| {
                (
                    "VARIABLE_RESOLUTION_FAILED",
                    "A configured value references a missing or invalid variable",
                )
            })?;
        let node_config = resolved_node_config
            .as_object()
            .cloned()
            .unwrap_or_default();
        let mut references = HashSet::new();
        collect_secret_references(&Value::Object(node_config.clone()), &mut references);
        let mut secrets = serde_json::Map::new();
        let mut replacements = HashMap::new();
        for reference in references {
            let secret = sqlx::query_as::<_, (String, String, String, i64)>(
                "SELECT id, name, encrypted_payload, secret_version FROM secrets WHERE workspace_id = ? AND (id = ? OR name = ?) LIMIT 1",
            )
            .bind(&workspace_id)
            .bind(&reference)
            .bind(&reference)
            .fetch_optional(&self.state.db)
            .await
            .ok()
            .flatten()
            .ok_or(("SECRET_UNAVAILABLE", "A required secret reference is unavailable"))?;
            let plaintext = crate::crypto::decrypt_secret(&self.state.config.master_key, &secret.2)
                .map_err(|_| {
                    (
                        "SECRET_UNAVAILABLE",
                        "A required secret could not be decrypted",
                    )
                })?;
            if sqlx::query(
                "INSERT INTO audit_events (id, actor_id, action, target_type, target_id, changes_json, created_at) VALUES (?, ?, 'secret.run_use', 'secret', ?, ?, ?)",
            )
            .bind(Uuid::new_v4().to_string())
            .bind(&actor_id)
            .bind(&secret.0)
            .bind(json!({ "run_id": run_id, "secret_version": secret.3 }).to_string())
            .bind(Utc::now().to_rfc3339())
            .execute(&self.state.db)
            .await
            .is_err()
            {
                return Err(("SECRET_AUDIT_FAILED", "The required secret use could not be recorded"));
            }
            secrets.insert(secret.1.clone(), json!(plaintext));
            replacements.insert(reference, secret.1);
        }
        let mut config = Value::Object(node_config);
        replace_secret_references(&mut config, &replacements);
        Ok((config, Value::Object(secrets)))
    }

    async fn load_case_definition(
        &self,
        case_id: &str,
        pinned_revision_id: Option<&str>,
    ) -> Option<(String, Value, String)> {
        let revision = sqlx::query_as::<_, (String, String, String)>(
            "SELECT id, definition_json, checksum FROM asset_revisions
             WHERE asset_id = ? AND (? IS NULL OR id = ?)
             ORDER BY version DESC LIMIT 1",
        )
        .bind(case_id)
        .bind(pinned_revision_id)
        .bind(pinned_revision_id)
        .fetch_optional(&self.state.db)
        .await
        .ok()??;
        let definition = serde_json::from_str(&revision.1).ok()?;
        Some((revision.0, definition, revision.2))
    }

    async fn run_is_canceled(&self, run_id: &str) -> bool {
        sqlx::query_scalar::<_, String>("SELECT status FROM suite_runs WHERE id = ?")
            .bind(run_id)
            .fetch_optional(&self.state.db)
            .await
            .ok()
            .flatten()
            .is_some_and(|status| status == "CANCELED")
    }

    async fn run_is_queued(&self, run_id: &str) -> bool {
        sqlx::query_scalar::<_, String>("SELECT status FROM suite_runs WHERE id = ?")
            .bind(run_id)
            .fetch_optional(&self.state.db)
            .await
            .ok()
            .flatten()
            .is_some_and(|status| status == "QUEUED")
    }

    async fn fail_run(&self, run_id: &str, reason: &str) {
        let now = Utc::now().to_rfc3339();
        let transitioned = sqlx::query(
            "UPDATE suite_runs SET status = 'ERROR', finished_at = ? WHERE id = ? AND status IN ('QUEUED', 'RUNNING')",
        )
            .bind(&now)
            .bind(run_id)
            .execute(&self.state.db)
            .await
            .is_ok_and(|result| result.rows_affected() == 1);
        self.release_resource_locks(run_id).await;

        if !transitioned {
            return;
        }

        self.emit_event(
            run_id,
            999,
            "run.finished",
            json!({ "run_id": run_id, "status": "ERROR", "error": reason }),
        )
        .await;
    }

    async fn emit_event(
        &self,
        run_id: &str,
        _requested_sequence: i64,
        event_type: &str,
        payload: Value,
    ) {
        let now = Utc::now().to_rfc3339();
        let event_id = Uuid::new_v4().to_string();
        let mut tx = match self.state.db.begin().await {
            Ok(tx) => tx,
            Err(error) => {
                warn!(
                    "Could not begin run-event transaction for {}: {}",
                    run_id, error
                );
                return;
            }
        };

        let seq: i64 = match sqlx::query_scalar(
            "SELECT COALESCE(MAX(sequence), 0) + 1 FROM run_events WHERE suite_run_id = ?",
        )
        .bind(run_id)
        .fetch_one(&mut *tx)
        .await
        {
            Ok(sequence) => sequence,
            Err(error) => {
                let _ = tx.rollback().await;
                warn!(
                    "Could not allocate a run-event sequence for {}: {}",
                    run_id, error
                );
                return;
            }
        };
        let mut event_payload = payload.as_object().cloned().unwrap_or_default();
        event_payload.insert("schema_version".to_string(), json!(1));
        event_payload.insert("run_id".to_string(), json!(run_id));
        event_payload.insert("sequence".to_string(), json!(seq));
        event_payload.insert("event_type".to_string(), json!(event_type));
        event_payload.insert("occurred_at".to_string(), json!(now));
        let payload_str = Value::Object(event_payload).to_string();

        let event_result = sqlx::query(
            "INSERT INTO run_events (id, suite_run_id, sequence, event_type, payload_json, occurred_at)
             VALUES (?, ?, ?, ?, ?, ?)"
        )
        .bind(&event_id)
        .bind(run_id)
        .bind(seq)
        .bind(event_type)
        .bind(&payload_str)
        .bind(&now)
        .execute(&mut *tx)
        .await;
        if let Err(error) = event_result {
            let _ = tx.rollback().await;
            warn!("Could not persist run event for {}: {}", run_id, error);
            return;
        }

        let workspace_id =
            sqlx::query_scalar::<_, String>("SELECT workspace_id FROM suite_runs WHERE id = ?")
                .bind(run_id)
                .fetch_optional(&mut *tx)
                .await
                .ok()
                .flatten()
                .unwrap_or_else(|| "unknown".to_string());
        let outbox_id = Uuid::new_v4().to_string();
        let subject = format!("automation.v1.{}.runs.{}.stats", workspace_id, run_id);
        let outbox_result = sqlx::query(
            "INSERT INTO nats_event_outbox (id, suite_run_id, sequence, subject, payload_json, status, created_at)
             VALUES (?, ?, ?, ?, ?, 'PENDING', ?)"
        )
        .bind(&outbox_id)
        .bind(run_id)
        .bind(seq)
        .bind(&subject)
        .bind(&payload_str)
        .bind(&now)
        .execute(&mut *tx)
        .await;
        if let Err(error) = outbox_result {
            let _ = tx.rollback().await;
            warn!(
                "Could not persist NATS outbox item for {}: {}",
                run_id, error
            );
            return;
        }

        let case_statuses = sqlx::query_as::<_, (String, i64)>(
            "SELECT status, COUNT(*) FROM case_runs WHERE suite_run_id = ? AND execution_scope = 'case' GROUP BY status",
        )
        .bind(run_id)
        .fetch_all(&mut *tx)
        .await
        .unwrap_or_default();
        let step_statuses = sqlx::query_as::<_, (String, i64)>(
            "SELECT steps.status, COUNT(*) FROM step_runs steps JOIN case_runs cases ON cases.id = steps.case_run_id WHERE cases.suite_run_id = ? GROUP BY steps.status",
        )
        .bind(run_id)
        .fetch_all(&mut *tx)
        .await
        .unwrap_or_default();
        let mut case_counts = serde_json::Map::new();
        for (status, count) in &case_statuses {
            case_counts.insert(status.to_ascii_lowercase(), json!(count));
        }
        let mut step_counts = serde_json::Map::new();
        for (status, count) in &step_statuses {
            step_counts.insert(status.to_ascii_lowercase(), json!(count));
        }
        let count_for = |rows: &[(String, i64)], statuses: &[&str]| -> i64 {
            rows.iter()
                .filter(|(status, _)| statuses.iter().any(|terminal| status == terminal))
                .map(|(_, count)| *count)
                .sum()
        };
        let terminal_cases = count_for(
            &case_statuses,
            &["PASSED", "FAILED", "ERROR", "SKIPPED", "CANCELED"],
        );
        let terminal_steps = count_for(
            &step_statuses,
            &[
                "SUCCEEDED",
                "ASSERTION_FAILED",
                "ERROR",
                "TIMED_OUT",
                "CANCELED",
                "SKIPPED",
                "INTERRUPTED",
            ],
        );
        let run_manifest = sqlx::query_scalar::<_, String>(
            "SELECT run_manifest_json FROM suite_runs WHERE id = ?",
        )
        .bind(run_id)
        .fetch_optional(&mut *tx)
        .await
        .ok()
        .flatten()
        .and_then(|value| serde_json::from_str::<Value>(&value).ok());
        let planned_cases = run_manifest
            .as_ref()
            .and_then(|manifest| {
                manifest
                    .get("planned_case_iterations")
                    .and_then(Value::as_i64)
            })
            .unwrap_or(0);
        let planned_steps = run_manifest
            .as_ref()
            .and_then(|manifest| {
                manifest
                    .get("planned_node_invocations")
                    .and_then(Value::as_i64)
            })
            .unwrap_or(0);
        let percent = if planned_cases > 0 {
            ((terminal_cases * 100) / planned_cases).clamp(0, 100) as u32
        } else {
            0
        };
        let current_case = sqlx::query_scalar::<_, String>(
            "SELECT CASE cases.execution_scope WHEN 'suite_setup' THEN 'Suite setup' WHEN 'suite_cleanup' THEN 'Suite cleanup' ELSE assets.name END FROM case_runs cases LEFT JOIN asset_revisions revisions ON revisions.id = cases.case_revision_id LEFT JOIN assets ON assets.id = revisions.asset_id WHERE cases.suite_run_id = ? AND cases.status = 'RUNNING' ORDER BY cases.started_at DESC LIMIT 1",
        )
        .bind(run_id)
        .fetch_optional(&mut *tx)
        .await
        .ok()
        .flatten();
        let current_step = sqlx::query_scalar::<_, String>(
            "SELECT steps.node_name FROM step_runs steps JOIN case_runs cases ON cases.id = steps.case_run_id WHERE cases.suite_run_id = ? AND steps.status = 'RUNNING' ORDER BY steps.started_at DESC LIMIT 1",
        )
        .bind(run_id)
        .fetch_optional(&mut *tx)
        .await
        .ok()
        .flatten();
        let snapshot_status =
            sqlx::query_scalar::<_, String>("SELECT status FROM suite_runs WHERE id = ?")
                .bind(run_id)
                .fetch_optional(&mut *tx)
                .await
                .ok()
                .flatten()
                .unwrap_or_else(|| "ERROR".to_string());
        let progress_json = json!({
            "mode": if planned_cases > 0 { "determinate" } else { "indeterminate" },
            "percent": percent,
            "terminal_nodes": terminal_steps,
            "planned_nodes": planned_steps,
            "terminal_cases": terminal_cases,
            "planned_cases": planned_cases
        });
        let stats_json = json!({
            "cases_total": planned_cases,
            "cases_completed": terminal_cases,
            "case_counts": Value::Object(case_counts),
            "node_counts": Value::Object(step_counts),
            "current_case": current_case,
            "current_step": current_step
        });
        let snapshot = sqlx::query(
            "UPDATE run_progress_snapshots SET status = ?, progress_json = ?, stats_json = ?, last_sequence = CASE WHEN last_sequence < ? THEN ? ELSE last_sequence END, updated_at = ? WHERE suite_run_id = ?",
        )
        .bind(snapshot_status)
        .bind(progress_json.to_string())
        .bind(stats_json.to_string())
        .bind(seq)
        .bind(seq)
        .bind(&now)
        .bind(run_id)
        .execute(&mut *tx)
        .await;
        if let Err(error) = snapshot {
            let _ = tx.rollback().await;
            warn!(
                "Could not update snapshot sequence for {}: {}",
                run_id, error
            );
            return;
        }
        if let Err(error) = tx.commit().await {
            warn!("Could not commit run event for {}: {}", run_id, error);
        }
    }
}

fn json_object_map(text: &str) -> HashMap<String, Value> {
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod resource_lock_tests {
    use super::Orchestrator;
    use crate::{config::AppConfig, models::SuiteRunRecord, AppState};
    use std::{path::PathBuf, sync::Arc};
    use uuid::Uuid;

    async fn test_orchestrator() -> (Orchestrator, PathBuf) {
        let database_path =
            std::env::temp_dir().join(format!("testit-resource-lock-{}.sqlite", Uuid::new_v4()));
        let database_url = format!(
            "sqlite://{}?mode=rwc",
            database_path.to_string_lossy().replace('\\', "/")
        );
        let db = crate::db::init_db(&database_url).await.unwrap();
        for statement in [
            "INSERT INTO workspaces (id, name) VALUES ('workspace', 'Test workspace')",
            "INSERT INTO users (id, workspace_id, email, display_name, role, password_hash) VALUES ('user', 'workspace', 'test@example.test', 'Test User', 'ADMIN', 'hash')",
            "INSERT INTO environments (id, workspace_id, name, variables_json) VALUES ('environment', 'workspace', 'Test', '{}')",
            "INSERT INTO assets (id, workspace_id, kind, name, draft_json) VALUES ('suite', 'workspace', 'suite', 'Test suite', '{}')",
            "INSERT INTO asset_revisions (id, asset_id, version, definition_json, checksum, author_id) VALUES ('suite-revision', 'suite', 1, '{\"cases\":[]}', 'checksum', 'user')",
        ] {
            sqlx::query(statement).execute(&db).await.unwrap();
        }
        for run_id in ["run-one", "run-two", "run-three", "run-four"] {
            sqlx::query(
                "INSERT INTO suite_runs (id, workspace_id, suite_revision_id, environment_id, status, run_manifest_json, random_seed)
                 VALUES (?, 'workspace', 'suite-revision', 'environment', 'QUEUED', '{}', 7)",
            )
            .bind(run_id)
            .execute(&db)
            .await
            .unwrap();
        }
        let config = AppConfig {
            port: 8080,
            database_url,
            nats_url: "nats://localhost:4222".to_string(),
            nats_control_user: None,
            nats_control_password: None,
            nats_sse_user: None,
            nats_sse_password: None,
            master_key: [7u8; 32],
            artifacts_dir: std::env::temp_dir().to_string_lossy().into_owned(),
            docker_worker_image: "test-worker".to_string(),
            max_active_cases: 2,
            bootstrap_admin_email: None,
            bootstrap_admin_password: None,
            cookie_secure: true,
            cors_allowed_origins: Vec::new(),
            worker_manager_url: None,
            worker_manager_token: None,
        };
        (
            Orchestrator::new(AppState {
                config: Arc::new(config),
                db,
                nats: None,
                nats_sse: None,
            }),
            database_path,
        )
    }

    async fn load_run(orchestrator: &Orchestrator, id: &str) -> SuiteRunRecord {
        sqlx::query_as::<_, SuiteRunRecord>("SELECT * FROM suite_runs WHERE id = ?")
            .bind(id)
            .fetch_one(&orchestrator.state.db)
            .await
            .unwrap()
    }

    async fn cleanup(orchestrator: Orchestrator, database_path: PathBuf) {
        orchestrator.state.db.close().await;
        let _ = tokio::fs::remove_file(&database_path).await;
        let _ = tokio::fs::remove_file(database_path.with_extension("sqlite-wal")).await;
        let _ = tokio::fs::remove_file(database_path.with_extension("sqlite-shm")).await;
    }

    #[tokio::test]
    async fn resource_locks_serialize_matching_names_and_release_after_terminal_runs() {
        let (orchestrator, database_path) = test_orchestrator().await;
        let first = load_run(&orchestrator, "run-one").await;
        let second = load_run(&orchestrator, "run-two").await;
        let third = load_run(&orchestrator, "run-three").await;
        let now = chrono::Utc::now().to_rfc3339();

        assert!(orchestrator
            .try_claim_run(&first, &["workspace|tenant:shared".to_string()], &now)
            .await
            .unwrap());
        assert!(!orchestrator
            .try_claim_run(&second, &["workspace|tenant:shared".to_string()], &now)
            .await
            .unwrap());
        assert!(orchestrator
            .try_claim_run(&third, &["workspace|tenant:other".to_string()], &now)
            .await
            .unwrap());

        sqlx::query("UPDATE suite_runs SET status = 'PASSED' WHERE id = 'run-one'")
            .execute(&orchestrator.state.db)
            .await
            .unwrap();
        assert!(orchestrator
            .try_claim_run(&second, &["workspace|tenant:shared".to_string()], &now)
            .await
            .unwrap());
        let recovered_release_audit: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM audit_events WHERE action = 'resource_lock.release_recovered' AND target_id = 'workspace|tenant:shared'",
        )
        .fetch_one(&orchestrator.state.db)
        .await
        .unwrap();
        assert_eq!(recovered_release_audit, 1);

        sqlx::query("UPDATE suite_runs SET status = 'PASSED' WHERE id = 'run-two'")
            .execute(&orchestrator.state.db)
            .await
            .unwrap();
        orchestrator.release_resource_locks("run-two").await;
        let remaining: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM resource_locks WHERE run_id = 'run-two'")
                .fetch_one(&orchestrator.state.db)
                .await
                .unwrap();
        assert_eq!(remaining, 0);
        let release_audits: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM audit_events WHERE action = 'resource_lock.release' AND target_id = 'workspace|tenant:shared'",
        )
        .fetch_one(&orchestrator.state.db)
        .await
        .unwrap();
        assert_eq!(release_audits, 1);
        cleanup(orchestrator, database_path).await;
    }

    #[tokio::test]
    async fn expired_lease_becomes_uncertain_instead_of_being_stolen() {
        let (orchestrator, database_path) = test_orchestrator().await;
        let first = load_run(&orchestrator, "run-one").await;
        let second = load_run(&orchestrator, "run-two").await;
        let now = chrono::Utc::now().to_rfc3339();
        let key = "workspace|account:shared".to_string();

        assert!(orchestrator
            .try_claim_run(&first, std::slice::from_ref(&key), &now)
            .await
            .unwrap());
        sqlx::query(
            "UPDATE resource_locks SET lease_expires_at = '2000-01-01 00:00:00' WHERE resource_key = ?",
        )
        .bind(&key)
        .execute(&orchestrator.state.db)
        .await
        .unwrap();
        assert!(!orchestrator
            .try_claim_run(&second, std::slice::from_ref(&key), &now)
            .await
            .unwrap());
        let status: String =
            sqlx::query_scalar("SELECT status FROM resource_locks WHERE resource_key = ?")
                .bind(&key)
                .fetch_one(&orchestrator.state.db)
                .await
                .unwrap();
        assert_eq!(status, "UNCERTAIN");
        assert!(!orchestrator
            .try_claim_run(&second, std::slice::from_ref(&key), &now)
            .await
            .unwrap());
        cleanup(orchestrator, database_path).await;
    }

    #[tokio::test]
    async fn a_lock_wait_failure_does_not_overwrite_a_cancellation() {
        let (orchestrator, database_path) = test_orchestrator().await;
        sqlx::query("UPDATE suite_runs SET status = 'CANCELED' WHERE id = 'run-one'")
            .execute(&orchestrator.state.db)
            .await
            .unwrap();
        assert!(!orchestrator.run_is_queued("run-one").await);

        orchestrator
            .fail_run("run-one", "resource lock wait timed out")
            .await;

        let status: String =
            sqlx::query_scalar("SELECT status FROM suite_runs WHERE id = 'run-one'")
                .fetch_one(&orchestrator.state.db)
                .await
                .unwrap();
        assert_eq!(status, "CANCELED");
        cleanup(orchestrator, database_path).await;
    }
}

pub(crate) fn case_dataset_rows(case_def: &Value) -> Result<Vec<Value>, String> {
    let Some(dataset) = case_def.get("data_set").or_else(|| case_def.get("dataset")) else {
        return Ok(vec![json!({})]);
    };
    let format = dataset
        .get("format")
        .and_then(Value::as_str)
        .unwrap_or("json")
        .to_ascii_lowercase();
    let content = dataset.get("content").and_then(Value::as_str);
    if content.is_some_and(|text| text.len() > 1_048_576)
        || serde_json::to_vec(dataset).map_or(true, |bytes| bytes.len() > 1_048_576)
    {
        return Err("A case data set exceeds the 1 MiB size limit".to_string());
    }
    let rows = match format.as_str() {
        "json" => {
            if let Some(rows) = dataset.get("rows").and_then(Value::as_array) {
                rows.clone()
            } else if let Some(content) = content {
                serde_json::from_str::<Value>(content)
                    .map_err(|_| "A JSON data set must contain a valid JSON array".to_string())?
                    .as_array()
                    .cloned()
                    .ok_or_else(|| "A JSON data set must contain an array of rows".to_string())?
            } else {
                return Err("A JSON data set requires a rows array".to_string());
            }
        }
        "csv" => parse_csv_dataset(
            content.ok_or_else(|| "A CSV data set requires text content".to_string())?,
        )?,
        _ => return Err("Case data set format must be JSON or CSV".to_string()),
    };
    if rows.is_empty() || rows.len() > 100 {
        return Err("A case data set must contain between 1 and 100 rows".to_string());
    }
    if rows
        .iter()
        .any(|row| !row.is_object() || contains_sensitive_key(row))
    {
        return Err(
            "Data-set rows must be objects and must not contain credential fields".to_string(),
        );
    }
    Ok(rows)
}

pub(crate) fn validate_case_data_set(case_def: &Value) -> Result<usize, String> {
    case_dataset_rows(case_def).map(|rows| rows.len())
}

fn contains_sensitive_key(value: &Value) -> bool {
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
                || contains_sensitive_key(value)
        }),
        Value::Array(items) => items.iter().any(contains_sensitive_key),
        _ => false,
    }
}

fn parse_csv_dataset(text: &str) -> Result<Vec<Value>, String> {
    let mut records = Vec::<Vec<String>>::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut chars = text.chars().peekable();
    let mut quoted = false;
    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => record.push(std::mem::take(&mut field)),
            '\n' if !quoted => {
                record.push(std::mem::take(&mut field));
                if record.iter().any(|value| !value.is_empty()) {
                    records.push(std::mem::take(&mut record));
                } else {
                    record.clear();
                }
            }
            '\r' if !quoted => {}
            _ => field.push(ch),
        }
        if records.len() > 101 {
            return Err("A CSV data set may contain at most 100 rows".to_string());
        }
    }
    if quoted {
        return Err("A CSV data set contains an unterminated quoted field".to_string());
    }
    record.push(field);
    if record.iter().any(|value| !value.is_empty()) {
        records.push(record);
    }
    if records.len() < 2 {
        return Err("A CSV data set requires a header and at least one row".to_string());
    }
    let headers = records.remove(0);
    let mut unique_headers = HashSet::new();
    if headers.iter().any(|header| {
        let header = header.trim();
        header.is_empty() || !unique_headers.insert(header.to_string())
    }) {
        return Err("CSV headers must be non-empty and unique".to_string());
    }
    let mut rows = Vec::with_capacity(records.len());
    for values in records {
        if values.len() != headers.len() {
            return Err(
                "Every CSV row must have the same number of columns as its header".to_string(),
            );
        }
        let object = headers
            .iter()
            .zip(values)
            .map(|(header, value)| (header.trim().to_string(), Value::String(value)))
            .collect();
        rows.push(Value::Object(object));
    }
    Ok(rows)
}
