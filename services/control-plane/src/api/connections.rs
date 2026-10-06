use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{
    api::auth::AuthenticatedUser,
    crypto::encrypt_secret,
    models::{ConnectionProfileRecord, EnvironmentRecord, SecretRecord},
    AppState,
};

#[derive(Deserialize)]
pub struct CreateEnvRequest {
    pub name: String,
    pub description: Option<String>,
    pub variables: Value,
}

#[derive(Deserialize)]
pub struct CreateConnectionRequest {
    pub name: String,
    pub connector_type: String,
    pub settings: Value,
    pub secret_refs: Option<Value>,
}

#[derive(Deserialize)]
pub struct CreateSecretRequest {
    pub name: String,
    pub plaintext: String,
}

pub async fn list_environments(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let list = sqlx::query_as::<_, EnvironmentRecord>(
        "SELECT * FROM environments WHERE workspace_id = ? ORDER BY created_at DESC",
    )
    .bind(&user.workspace_id)
    .fetch_all(&state.db)
    .await;

    match list {
        Ok(envs) => (StatusCode::OK, Json(json!(envs))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

pub async fn create_environment(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(payload): Json<CreateEnvRequest>,
) -> impl IntoResponse {
    if !payload.variables.is_object() || contains_inline_sensitive_value(&payload.variables) {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                json!({ "error": "Environment variables must be an object and cannot contain inline credentials" }),
            ),
        );
    }
    if payload.name.trim().is_empty() || payload.name.chars().count() > 128 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Environment name must be between 1 and 128 characters" })),
        );
    }
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let desc = payload.description.unwrap_or_default();
    let vars_str = payload.variables.to_string();

    let mut tx = match state.db.begin().await {
        Ok(tx) => tx,
        Err(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": "Environment service is temporarily unavailable" })),
            )
        }
    };
    let res = sqlx::query(
        "INSERT INTO environments (id, workspace_id, name, description, variables_json, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&user.workspace_id)
    .bind(&payload.name)
    .bind(&desc)
    .bind(&vars_str)
    .bind(&now)
    .execute(&mut *tx)
    .await;

    if res.is_err()
        || record_admin_audit(
            &mut tx,
            &user.id,
            "environment.create",
            "environment",
            &id,
            json!({ "name": payload.name }),
            &now,
        )
        .await
        .is_err()
    {
        let _ = tx.rollback().await;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Could not save the environment" })),
        );
    }
    if tx.commit().await.is_err() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Could not commit the environment" })),
        );
    }
    (
        StatusCode::CREATED,
        Json(json!({ "id": id, "name": payload.name, "created_at": now })),
    )
}

pub async fn list_connections(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let list = sqlx::query_as::<_, ConnectionProfileRecord>(
        "SELECT * FROM connection_profiles WHERE workspace_id = ? ORDER BY created_at DESC",
    )
    .bind(&user.workspace_id)
    .fetch_all(&state.db)
    .await;

    match list {
        Ok(conns) => (StatusCode::OK, Json(json!(conns))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

pub async fn create_connection(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(payload): Json<CreateConnectionRequest>,
) -> impl IntoResponse {
    if payload.name.trim().is_empty() || payload.name.chars().count() > 128 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Connection name must be between 1 and 128 characters" })),
        );
    }
    if !matches!(
        payload.connector_type.as_str(),
        "mysql" | "mongodb" | "api" | "http" | "parquet" | "delta"
    ) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Unsupported connection type" })),
        );
    }
    if !payload.settings.is_object() || contains_inline_sensitive_value(&payload.settings) {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                json!({ "error": "Connection settings must be an object without inline credentials" }),
            ),
        );
    }
    let secret_refs = payload
        .secret_refs
        .as_ref()
        .cloned()
        .unwrap_or_else(|| json!({}));
    let Some(refs) = secret_refs.as_object() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Secret references must be a JSON object" })),
        );
    };
    for reference in refs.values() {
        let Some(reference) = reference.as_str() else {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "Each secret reference must be a secret ID or name" })),
            );
        };
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM secrets WHERE workspace_id = ? AND (id = ? OR name = ?))",
        )
        .bind(&user.workspace_id)
        .bind(reference)
        .bind(reference)
        .fetch_one(&state.db)
        .await
        .unwrap_or(false);
        if !exists {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "A secret reference is unavailable in this workspace" })),
            );
        }
    }
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let settings_str = payload.settings.to_string();
    let sec_refs_str = payload
        .secret_refs
        .map(|s| s.to_string())
        .unwrap_or_else(|| "{}".to_string());

    let mut tx = match state.db.begin().await {
        Ok(tx) => tx,
        Err(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": "Connection service is temporarily unavailable" })),
            )
        }
    };
    let res = sqlx::query(
        "INSERT INTO connection_profiles (id, workspace_id, name, connector_type, settings_json, secret_refs_json, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&id)
    .bind(&user.workspace_id)
    .bind(&payload.name)
    .bind(&payload.connector_type)
    .bind(&settings_str)
    .bind(&sec_refs_str)
    .bind(&now)
    .execute(&mut *tx)
    .await;

    if res.is_err()
        || record_admin_audit(
            &mut tx,
            &user.id,
            "connection.create",
            "connection_profile",
            &id,
            json!({ "name": payload.name, "connector_type": payload.connector_type }),
            &now,
        )
        .await
        .is_err()
    {
        let _ = tx.rollback().await;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Could not save the connection profile" })),
        );
    }
    if tx.commit().await.is_err() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Could not commit the connection profile" })),
        );
    }
    (
        StatusCode::CREATED,
        Json(
            json!({ "id": id, "name": payload.name, "connector_type": payload.connector_type, "created_at": now }),
        ),
    )
}

pub async fn update_connection(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(payload): Json<CreateConnectionRequest>,
) -> impl IntoResponse {
    if payload.name.trim().is_empty() || payload.name.chars().count() > 128 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Connection name must be between 1 and 128 characters" })),
        );
    }
    if !matches!(
        payload.connector_type.as_str(),
        "mysql" | "mongodb" | "api" | "http" | "parquet" | "delta"
    ) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Unsupported connection type" })),
        );
    }
    if !payload.settings.is_object() || contains_inline_sensitive_value(&payload.settings) {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                json!({ "error": "Connection settings must be an object without inline credentials" }),
            ),
        );
    }
    let secret_refs = payload
        .secret_refs
        .as_ref()
        .cloned()
        .unwrap_or_else(|| json!({}));
    let Some(references) = secret_refs.as_object() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Secret references must be a JSON object" })),
        );
    };
    for value in references.values() {
        let Some(reference) = value
            .as_str()
            .filter(|reference| !reference.trim().is_empty())
        else {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "Each secret reference must be a secret ID or name" })),
            );
        };
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM secrets WHERE workspace_id = ? AND (id = ? OR name = ?))",
        )
        .bind(&user.workspace_id)
        .bind(reference)
        .bind(reference)
        .fetch_one(&state.db)
        .await
        .unwrap_or(false);
        if !exists {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "A secret reference is unavailable in this workspace" })),
            );
        }
    }
    let now = Utc::now().to_rfc3339();
    let mut tx = match state.db.begin().await {
        Ok(tx) => tx,
        Err(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": "Connection service is temporarily unavailable" })),
            )
        }
    };
    let updated = sqlx::query(
        "UPDATE connection_profiles SET name = ?, connector_type = ?, settings_json = ?, secret_refs_json = ?
         WHERE id = ? AND workspace_id = ?",
    )
    .bind(&payload.name)
    .bind(&payload.connector_type)
    .bind(payload.settings.to_string())
    .bind(secret_refs.to_string())
    .bind(&id)
    .bind(&user.workspace_id)
    .execute(&mut *tx)
    .await;
    match updated {
        Ok(result) if result.rows_affected() == 1 => {}
        Ok(_) => {
            let _ = tx.rollback().await;
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "Connection profile was not found" })),
            );
        }
        Err(_) => {
            let _ = tx.rollback().await;
            return (
                StatusCode::CONFLICT,
                Json(json!({ "error": "Could not update this connection profile" })),
            );
        }
    }
    if record_admin_audit(
        &mut tx,
        &user.id,
        "connection.update",
        "connection_profile",
        &id,
        json!({ "name": payload.name, "connector_type": payload.connector_type }),
        &now,
    )
    .await
    .is_err()
        || tx.commit().await.is_err()
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Could not record the connection profile update" })),
        );
    }
    (
        StatusCode::OK,
        Json(json!({ "id": id, "name": payload.name, "status": "SAVED", "updated_at": now })),
    )
}

pub async fn test_connection(
    State(state): State<AppState>,
    Path(conn_id): Path<String>,
    Extension(user): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let conn = sqlx::query_as::<_, ConnectionProfileRecord>(
        "SELECT * FROM connection_profiles WHERE id = ? AND workspace_id = ?",
    )
    .bind(&conn_id)
    .bind(&user.workspace_id)
    .fetch_optional(&state.db)
    .await;

    match conn {
        Ok(Some(c)) => run_connection_probe(&state, &user, &c).await,
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Connection profile not found" })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

async fn run_connection_probe(
    state: &AppState,
    user: &AuthenticatedUser,
    profile: &ConnectionProfileRecord,
) -> (StatusCode, Json<Value>) {
    let (Some(url), Some(token)) = (
        state.config.worker_manager_url.as_deref(),
        state.config.worker_manager_token.as_deref(),
    ) else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "The isolated worker engine is unavailable" })),
        );
    };
    let node_type = match profile.connector_type.as_str() {
        "mysql" => "db.mysql",
        "mongodb" => "db.mongodb",
        "api" | "http" => "api.request",
        "parquet" | "delta" => "data.tabular",
        _ => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({ "error": "This connection type does not have an executable probe" })),
            )
        }
    };
    let mut config: serde_json::Map<String, Value> =
        match serde_json::from_str(&profile.settings_json) {
            Ok(settings) => settings,
            Err(_) => {
                return (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(json!({ "error": "Connection profile settings are invalid" })),
                )
            }
        };
    if contains_inline_sensitive_value(&Value::Object(config.clone())) {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(
                json!({ "error": "Connection profiles cannot store credentials inline; use secret references" }),
            ),
        );
    }
    if let Ok(refs) = serde_json::from_str::<Value>(&profile.secret_refs_json) {
        if let Some(refs) = refs.as_object() {
            for (key, reference) in refs {
                let alias = if key.to_ascii_lowercase().ends_with("_secret") {
                    key.clone()
                } else {
                    format!("{}_secret", key)
                };
                config.insert(alias, reference.clone());
            }
        }
    }
    if node_type == "db.mysql" {
        config.insert("query".into(), json!("SELECT 1 AS testit_health_check"));
        config.insert("output_columns".into(), json!(["testit_health_check"]));
        config.insert("expected_min_rows".into(), json!(1));
    } else if node_type == "db.mongodb" {
        config.insert("health_check".into(), json!(true));
    } else if node_type == "api.request" {
        if let (Some(base), Some(path)) = (
            config
                .get("base_url")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            config
                .get("path")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
        ) {
            config.entry("url").or_insert_with(|| {
                json!(format!(
                    "{}{}{}",
                    base.trim_end_matches('/'),
                    if path.starts_with('/') { "" } else { "/" },
                    path
                ))
            });
        }
        config.entry("expected_status").or_insert(json!(200));
        config.insert("allowed_status_codes".into(), json!([200, 201, 202, 204]));
    }

    let mut references = std::collections::HashSet::new();
    collect_secret_references(&Value::Object(config.clone()), &mut references);
    let mut replacements = std::collections::HashMap::new();
    let mut secrets = serde_json::Map::new();
    for reference in references {
        let record = sqlx::query_as::<_, (String, String, String, i64)>(
            "SELECT id, name, encrypted_payload, secret_version FROM secrets WHERE workspace_id = ? AND (id = ? OR name = ?) LIMIT 1",
        )
        .bind(&user.workspace_id)
        .bind(&reference)
        .bind(&reference)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();
        let Some((secret_id, name, encrypted, secret_version)) = record else {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({ "error": "A connection secret reference is unavailable" })),
            );
        };
        let plaintext = match crate::crypto::decrypt_secret(&state.config.master_key, &encrypted) {
            Ok(value) => value,
            Err(_) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": "A connection secret could not be decrypted" })),
                )
            }
        };
        if sqlx::query(
            "INSERT INTO audit_events (id, actor_id, action, target_type, target_id, changes_json, created_at) VALUES (?, ?, 'secret.connection_test_use', 'secret', ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(&user.id)
        .bind(secret_id)
        .bind(json!({ "connection_id": profile.id, "secret_version": secret_version }).to_string())
        .bind(Utc::now().to_rfc3339())
        .execute(&state.db)
        .await
        .is_err()
        {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Could not record the connection secret access" })),
            );
        }
        secrets.insert(name.clone(), json!(plaintext));
        replacements.insert(reference, name);
    }
    let mut resolved_config = Value::Object(config);
    replace_secret_references(&mut resolved_config, &replacements);
    if record_audit_event(
        &state.db,
        &user.id,
        "connection.test",
        "connection_profile",
        &profile.id,
        json!({ "connector_type": profile.connector_type }),
    )
    .await
    .is_err()
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Could not record the connection test" })),
        );
    }
    let run_id = Uuid::new_v4().to_string();
    let step_id = Uuid::new_v4().to_string();
    let envelope = json!({
        "schema_version": 1,
        "run_id": run_id,
        "case_id": Uuid::new_v4().to_string(),
        "step_id": step_id,
        "attempt": 1,
        "deadline_utc": (Utc::now() + chrono::Duration::seconds(35)).to_rfc3339(),
        "node_type": node_type,
        "node_type_version": 1,
        "config": resolved_config,
        "inputs": {},
        "secrets": Value::Object(secrets),
        "limits": { "max_output_bytes": 262144, "max_log_bytes": 65536, "timeout_seconds": 30 }
    });
    let started = std::time::Instant::now();
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(40))
        .build()
    {
        Ok(client) => client,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Could not initialize the isolated connection test" })),
            )
        }
    };
    let response = match client
        .post(format!("{}/v1/invoke", url))
        .bearer_auth(token)
        .json(&envelope)
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": "The isolated worker engine could not be reached" })),
            )
        }
    };
    if !response.status().is_success() {
        return (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": "The isolated worker rejected the connection test" })),
        );
    }
    let result: Value = match response.json().await {
        Ok(result) => result,
        Err(_) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(
                    json!({ "error": "The isolated worker returned an invalid connection-test result" }),
                ),
            )
        }
    };
    if result["result"]["step_id"].as_str() != Some(step_id.as_str()) {
        return (
            StatusCode::BAD_GATEWAY,
            Json(
                json!({ "error": "The isolated worker returned a mismatched connection-test result" }),
            ),
        );
    }
    let succeeded = result["result"]["status"].as_str() == Some("SUCCEEDED");
    let latency_ms = started.elapsed().as_secs_f64() * 1000.0;
    (
        if succeeded {
            StatusCode::OK
        } else {
            StatusCode::BAD_GATEWAY
        },
        Json(json!({
            "connection_id": profile.id,
            "connector_type": profile.connector_type,
            "status": if succeeded { "SUCCESS" } else { "FAILED" },
            "message": if succeeded { "The connection test completed successfully" } else { "The connection test failed; check the profile and worker egress allow-list" },
            "latency_ms": latency_ms
        })),
    )
}

pub async fn list_secrets(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let list = sqlx::query_as::<_, SecretRecord>(
        "SELECT id, workspace_id, name, '***ENCRYPTED***' as encrypted_payload, key_version, secret_version, created_at, updated_at 
         FROM secrets WHERE workspace_id = ? ORDER BY name ASC"
    )
    .bind(&user.workspace_id)
    .fetch_all(&state.db)
    .await;

    match list {
        Ok(secs) => (StatusCode::OK, Json(json!(secs))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

pub async fn create_secret(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(payload): Json<CreateSecretRequest>,
) -> impl IntoResponse {
    if payload.name.trim().is_empty()
        || payload.name.len() > 128
        || payload.plaintext.is_empty()
        || payload.plaintext.len() > 65536
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                json!({ "error": "Secret name and value are required; values are limited to 64 KiB" }),
            ),
        );
    }
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    let encrypted = match encrypt_secret(&state.config.master_key, &payload.plaintext) {
        Ok(enc) => enc,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("Encryption failure: {}", e) })),
            )
        }
    };

    let mut tx = match state.db.begin().await {
        Ok(tx) => tx,
        Err(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": "Secret storage is temporarily unavailable" })),
            )
        }
    };
    let res = sqlx::query(
        "INSERT INTO secrets (id, workspace_id, name, encrypted_payload, key_version, secret_version, created_at, updated_at)
         VALUES (?, ?, ?, ?, 1, 1, ?, ?)
         ON CONFLICT(name) DO UPDATE SET encrypted_payload = excluded.encrypted_payload, secret_version = secret_version + 1, updated_at = excluded.updated_at
         WHERE secrets.workspace_id = excluded.workspace_id"
    )
    .bind(&id)
    .bind(&user.workspace_id)
    .bind(&payload.name)
    .bind(&encrypted)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await;

    match res {
        Ok(result) if result.rows_affected() == 0 => {
            let _ = tx.rollback().await;
            return (
                StatusCode::CONFLICT,
                Json(
                    json!({ "error": "A secret with this name already exists in another workspace" }),
                ),
            );
        }
        Ok(_) => {}
        Err(_) => {
            let _ = tx.rollback().await;
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Could not save the secret" })),
            );
        }
    }
    if record_admin_audit(
        &mut tx,
        &user.id,
        "secret.upsert",
        "secret",
        &payload.name,
        json!({ "name": &payload.name }),
        &now,
    )
    .await
    .is_err()
    {
        let _ = tx.rollback().await;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Could not record the secret update" })),
        );
    }
    if tx.commit().await.is_err() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Could not commit the secret update" })),
        );
    }
    (
        StatusCode::CREATED,
        Json(json!({ "name": payload.name, "status": "SAVED", "updated_at": now })),
    )
}

fn contains_inline_sensitive_value(value: &Value) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(key, nested)| {
            let key = key.to_ascii_lowercase();
            let reference_field =
                key.ends_with("_secret") || key == "secret_ref" || key == "secret_refs";
            let sensitive_field = [
                "password",
                "token",
                "authorization",
                "cookie",
                "api_key",
                "apikey",
                "credential",
            ]
            .iter()
            .any(|marker| key.contains(marker));
            let embedded_url_secret = (key == "uri" || key.ends_with("_url") || key == "url")
                .then(|| nested.as_str())
                .flatten()
                .is_some_and(url_contains_credentials);
            (sensitive_field && !reference_field && !nested.is_null())
                || embedded_url_secret
                || contains_inline_sensitive_value(nested)
        }),
        Value::Array(values) => values.iter().any(contains_inline_sensitive_value),
        _ => false,
    }
}

fn url_contains_credentials(raw: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(raw) else {
        return false;
    };
    if !url.username().is_empty() || url.password().is_some() {
        return true;
    }
    url.query_pairs().any(|(key, _)| {
        let key = key.to_ascii_lowercase();
        [
            "token",
            "key",
            "password",
            "secret",
            "credential",
            "authorization",
            "cookie",
        ]
        .iter()
        .any(|marker| key.contains(marker))
    })
}

async fn record_admin_audit(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    actor_id: &str,
    action: &str,
    target_type: &str,
    target_id: &str,
    changes: Value,
    at: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO audit_events (id, actor_id, action, target_type, target_id, changes_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(actor_id)
    .bind(action)
    .bind(target_type)
    .bind(target_id)
    .bind(changes.to_string())
    .bind(at)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn record_audit_event(
    db: &crate::db::DbPool,
    actor_id: &str,
    action: &str,
    target_type: &str,
    target_id: &str,
    changes: Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO audit_events (id, actor_id, action, target_type, target_id, changes_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(actor_id)
    .bind(action)
    .bind(target_type)
    .bind(target_id)
    .bind(changes.to_string())
    .bind(Utc::now().to_rfc3339())
    .execute(db)
    .await?;
    Ok(())
}

fn collect_secret_references(value: &Value, references: &mut std::collections::HashSet<String>) {
    match value {
        Value::Object(object) => {
            for (key, nested) in object {
                let normalized = key.to_ascii_lowercase();
                if (normalized.ends_with("_secret") || normalized == "secret_ref")
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

fn replace_secret_references(
    value: &mut Value,
    replacements: &std::collections::HashMap<String, String>,
) {
    match value {
        Value::Object(object) => {
            for (key, nested) in object {
                let normalized = key.to_ascii_lowercase();
                if normalized.ends_with("_secret") || normalized == "secret_ref" {
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
