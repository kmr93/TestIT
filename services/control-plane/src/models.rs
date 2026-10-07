use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub id: String,
    pub workspace_id: String,
    pub email: String,
    pub display_name: String,
    pub role: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Asset {
    pub id: String,
    pub workspace_id: String,
    pub kind: String,
    pub name: String,
    pub description: String,
    pub draft_json: String,
    pub draft_version: i64,
    pub archived_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct AssetRevision {
    pub id: String,
    pub asset_id: String,
    pub version: i64,
    pub definition_json: String,
    pub checksum: String,
    pub change_note: String,
    pub author_id: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct EnvironmentRecord {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub description: String,
    pub variables_json: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct ConnectionProfileRecord {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub connector_type: String,
    pub settings_json: String,
    pub secret_refs_json: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct SecretRecord {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub encrypted_payload: String,
    pub key_version: i64,
    pub secret_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct SuiteRunRecord {
    pub id: String,
    pub workspace_id: String,
    pub suite_revision_id: String,
    pub environment_id: String,
    pub status: String,
    pub initiating_user_id: Option<String>,
    pub idempotency_key: Option<String>,
    pub run_manifest_json: String,
    pub random_seed: i64,
    pub catalog_version: String,
    pub inputs_json: String,
    pub variable_overrides_json: String,
    pub source_run_id: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct CaseRunRecord {
    pub id: String,
    pub suite_run_id: String,
    pub case_revision_id: String,
    pub execution_scope: String,
    pub ordinal: i64,
    pub iteration_index: i64,
    pub status: String,
    pub inputs_json: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct StepRunRecord {
    pub id: String,
    pub case_run_id: String,
    pub node_id: String,
    pub node_name: String,
    pub node_type: String,
    pub ordinal: i64,
    pub attempt: i64,
    pub status: String,
    pub duration_ms: Option<f64>,
    pub error_json: Option<String>,
    pub outputs_json: Option<String>,
    pub metrics_json: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct RunProgressSnapshotRecord {
    pub suite_run_id: String,
    pub status: String,
    pub progress_json: String,
    pub stats_json: String,
    pub last_sequence: i64,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct RunEventRecord {
    pub id: String,
    pub suite_run_id: String,
    pub sequence: i64,
    pub event_type: String,
    pub payload_json: String,
    pub occurred_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct NatsEventOutboxRecord {
    pub id: String,
    pub suite_run_id: String,
    pub sequence: i64,
    pub subject: String,
    pub payload_json: String,
    pub status: String,
    pub attempts: i64,
    pub next_attempt_at: String,
    pub created_at: String,
}

// ======================== API DTOs ========================

#[derive(Clone, Debug, Deserialize)]
pub struct PublishDraftRequest {
    pub expected_draft_version: i64,
    pub change_note: Option<String>,
    pub dependency_update_policy: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PublishDraftResponse {
    pub revision_id: String,
    pub version: i64,
    pub checksum: String,
    pub published_at: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TriggerRunRequest {
    pub suite_revision_id: String,
    pub environment_id: String,
    pub inputs: Option<Value>,
    pub variable_overrides: Option<Value>,
    pub notification: Option<Value>,
    pub idempotency_key: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TriggerRunResponse {
    pub run_id: String,
    pub status: String,
    pub created_at: String,
    pub links: RunLinks,
}

#[derive(Clone, Debug, Serialize)]
pub struct RunLinks {
    pub status: String,
    pub events: String,
    pub report: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct VariablePreviewRequest {
    pub node_config: Value,
    pub environment_id: Option<String>,
    pub sample_inputs: Option<Value>,
    pub sample_iteration: Option<Value>,
    pub suite_variables: Option<Value>,
    pub case_variables: Option<Value>,
}

#[derive(Clone, Debug, Serialize)]
pub struct VariablePreviewResponse {
    pub network_accessed: bool,
    pub resolved_config: Value,
    pub masked_secrets: Vec<String>,
    pub errors: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProgressInfo {
    pub mode: String, // "determinate" or "indeterminate"
    pub percent: Option<u32>,
    pub terminal_nodes: usize,
    pub planned_nodes: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunStatsSummary {
    pub run_id: String,
    pub status: String,
    pub progress: ProgressInfo,
    pub case_counts: Value,
    pub node_counts: Value,
    pub elapsed_seconds: f64,
    pub current_case: Option<String>,
    pub current_step: Option<String>,
    pub last_updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ComparisonItem {
    pub name: String,
    pub baseline_status: Option<String>,
    pub current_status: String,
    pub baseline_duration_ms: Option<f64>,
    pub current_duration_ms: Option<f64>,
    pub duration_delta_ms: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunComparisonResponse {
    pub current_run_id: String,
    pub baseline_run_id: Option<String>,
    pub baseline_found: bool,
    pub reason: Option<String>,
    pub summary_delta: Value,
    pub case_comparisons: Vec<ComparisonItem>,
}
