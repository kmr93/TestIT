use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use chrono::Utc;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::{api::auth::AuthenticatedUser, crypto::compute_sha256, AppState};

const MAX_ARCHIVE_BYTES: usize = 10 * 1024 * 1024;
const MAX_EXPANDED_BYTES: usize = 32 * 1024 * 1024;
const MAX_ARCHIVE_FILES: usize = 4;
const BUNDLE_FORMAT_VERSION: u64 = 1;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/portability/export", get(export_bundle))
        .route(
            "/api/v1/portability/preview",
            post(preview_bundle).layer(DefaultBodyLimit::max(MAX_ARCHIVE_BYTES)),
        )
        .route(
            "/api/v1/portability/import",
            post(import_bundle).layer(DefaultBodyLimit::max(MAX_ARCHIVE_BYTES)),
        )
}

async fn export_bundle(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    let secret_rows = match sqlx::query_scalar::<_, String>(
        "SELECT encrypted_payload FROM secrets WHERE workspace_id = ? LIMIT 1001",
    )
    .bind(&user.workspace_id)
    .fetch_all(&state.db)
    .await
    {
        Ok(rows) if rows.len() <= 1000 => rows,
        Ok(_) => {
            return api_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "Too many workspace secrets to safely create a portable bundle",
            )
        }
        Err(_) => {
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Could not verify workspace secrets before export",
            )
        }
    };
    let mut known_secrets = Vec::with_capacity(secret_rows.len());
    for encrypted in secret_rows {
        let secret = match crate::crypto::decrypt_secret(&state.config.master_key, &encrypted) {
            Ok(secret) => secret,
            Err(_) => {
                return api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Could not verify workspace secrets before export",
                )
            }
        };
        if !secret.is_empty() {
            known_secrets.push(secret);
        }
    }
    known_secrets.sort_by_key(|value| std::cmp::Reverse(value.len()));
    known_secrets.dedup();

    let asset_rows = match sqlx::query_as::<_, (String, String, String, String)>(
        "SELECT id, kind, name, description FROM assets
         WHERE workspace_id = ? AND kind IN ('suite', 'case')
           AND EXISTS (SELECT 1 FROM asset_revisions WHERE asset_revisions.asset_id = assets.id)
         ORDER BY kind, name LIMIT 1001",
    )
    .bind(&user.workspace_id)
    .fetch_all(&state.db)
    .await
    {
        Ok(rows) if rows.len() <= 1000 => rows,
        Ok(_) => {
            return api_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "A bundle can contain at most 1000 assets",
            )
        }
        Err(_) => {
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Could not read workspace assets",
            )
        }
    };

    let mut assets = Vec::with_capacity(asset_rows.len());
    let mut revision_count = 0usize;
    for (id, kind, name, description) in asset_rows {
        let revisions = match sqlx::query_as::<_, (String, i64, String, String)>(
            "SELECT id, version, definition_json, change_note FROM asset_revisions
             WHERE asset_id = ? ORDER BY version LIMIT 10001",
        )
        .bind(&id)
        .fetch_all(&state.db)
        .await
        {
            Ok(rows) if rows.len() <= 10_000 => rows,
            Ok(_) => {
                return api_error(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "A bundle can contain at most 10000 revisions",
                )
            }
            Err(_) => {
                return api_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Could not read published asset revisions",
                )
            }
        };
        revision_count += revisions.len();
        let mut revision_values = Vec::with_capacity(revisions.len());
        for (revision_id, version, definition_json, change_note) in revisions {
            let mut definition = match serde_json::from_str::<Value>(&definition_json) {
                Ok(value) => value,
                Err(_) => {
                    return api_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "A published definition is unreadable",
                    )
                }
            };
            scrub_sensitive_values(&mut definition);
            redact_known_secrets(&mut definition, &known_secrets);
            revision_values.push(json!({
                "id": revision_id,
                "version": version,
                "change_note": change_note,
                "definition": definition
            }));
        }
        assets.push(json!({
            "id": id,
            "kind": kind,
            "name": name,
            "description": description,
            "revisions": revision_values
        }));
    }
    for asset in &mut assets {
        scrub_sensitive_values(asset);
        redact_known_secrets(asset, &known_secrets);
    }

    let connection_rows = match sqlx::query_as::<_, (String, String, String, String, String)>(
        "SELECT id, name, connector_type, settings_json, secret_refs_json FROM connection_profiles
         WHERE workspace_id = ? ORDER BY name LIMIT 201",
    )
    .bind(&user.workspace_id)
    .fetch_all(&state.db)
    .await
    {
        Ok(rows) if rows.len() <= 200 => rows,
        Ok(_) => {
            return api_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "A bundle can contain at most 200 connection profiles",
            )
        }
        Err(_) => {
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Could not read connection profile definitions",
            )
        }
    };
    let mut connections = Vec::with_capacity(connection_rows.len());
    for (id, name, connector_type, settings_json, secret_refs_json) in connection_rows {
        let mut settings =
            serde_json::from_str::<Value>(&settings_json).unwrap_or_else(|_| json!({}));
        scrub_connection_settings(&mut settings);
        redact_known_secrets(&mut settings, &known_secrets);
        let needs_reentry = contains_reentry_marker(&settings);
        let secret_refs = serde_json::from_str::<Value>(&secret_refs_json)
            .ok()
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default()
            .into_iter()
            .map(|(key, _)| (key, json!("[REENTER_SECRET]")))
            .collect::<serde_json::Map<_, _>>();
        connections.push(json!({
            "id": id,
            "name": name,
            "connector_type": connector_type,
            "settings": settings,
            "secret_refs": Value::Object(secret_refs),
            "secrets_need_reentry": needs_reentry
        }));
    }
    for connection in &mut connections {
        scrub_sensitive_values(connection);
        redact_known_secrets(connection, &known_secrets);
    }

    let mut files = HashMap::<String, Vec<u8>>::new();
    files.insert(
        "definitions/assets.json".to_string(),
        serde_json::to_vec(&json!({ "assets": assets })).unwrap_or_default(),
    );
    files.insert(
        "definitions/connections.json".to_string(),
        serde_json::to_vec(&json!({ "connections": connections })).unwrap_or_default(),
    );
    let file_hashes = files
        .iter()
        .map(|(name, bytes)| (name.clone(), json!(compute_sha256(bytes))))
        .collect::<serde_json::Map<_, _>>();
    let manifest = serde_json::to_vec(&json!({
        "format_version": BUNDLE_FORMAT_VERSION,
        "minimum_app_version": "0.1.0",
        "created_at": Utc::now().to_rfc3339(),
        "asset_count": asset_rows_count_hint(&files),
        "revision_count": revision_count,
        "files": file_hashes
    }))
    .unwrap_or_default();
    let checksums = files
        .iter()
        .map(|(name, bytes)| format!("{}  {}\n", compute_sha256(bytes), name))
        .collect::<String>()
        .into_bytes();
    let mut entries = files;
    entries.insert("manifest.json".to_string(), manifest);
    entries.insert("checksums.sha256".to_string(), checksums);
    let archive = match write_stored_zip(entries) {
        Ok(bytes) if bytes.len() <= MAX_ARCHIVE_BYTES => bytes,
        Ok(_) => {
            return api_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "The project bundle exceeds 10 MiB",
            )
        }
        Err(_) => {
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not create the project bundle",
            )
        }
    };
    let audit = sqlx::query(
        "INSERT INTO audit_events (id, actor_id, action, target_type, target_id, changes_json, created_at)
         VALUES (?, ?, 'bundle.export', 'workspace', ?, ?, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(&user.id)
    .bind(&user.workspace_id)
    .bind(json!({ "asset_count": asset_rows_count_hint_from_values(&assets), "revision_count": revision_count, "connection_count": connections.len(), "format_version": BUNDLE_FORMAT_VERSION }).to_string())
    .bind(Utc::now().to_rfc3339())
    .execute(&state.db)
    .await;
    if audit.is_err() {
        return api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not record the project export",
        );
    }
    let mut response = archive.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=TestIT-project.zip"),
    );
    response
}

fn asset_rows_count_hint(files: &HashMap<String, Vec<u8>>) -> usize {
    files
        .get("definitions/assets.json")
        .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok())
        .and_then(|value| value.get("assets").and_then(Value::as_array).map(Vec::len))
        .unwrap_or(0)
}

fn asset_rows_count_hint_from_values(assets: &[Value]) -> usize {
    assets.len()
}

fn contains_reentry_marker(value: &Value) -> bool {
    match value {
        Value::String(text) => text.contains("[REENTER_") || text.contains("[REDACTED_SECRET]"),
        Value::Array(items) => items.iter().any(contains_reentry_marker),
        Value::Object(items) => items.values().any(contains_reentry_marker),
        _ => false,
    }
}

fn connection_needs_reentry(connection: &Value) -> bool {
    connection["secrets_need_reentry"]
        .as_bool()
        .unwrap_or(false)
        || contains_reentry_marker(&connection["settings"])
        || connection["secret_refs"]
            .as_object()
            .is_some_and(|references| !references.is_empty())
}

fn contains_unredacted_connection_secret(value: &Value) -> bool {
    match value {
        Value::Object(items) => items.iter().any(|(key, nested)| {
            let key = key.to_ascii_lowercase();
            let sensitive = [
                "password",
                "secret",
                "token",
                "authorization",
                "cookie",
                "credential",
                "api_key",
                "apikey",
            ]
            .iter()
            .any(|marker| key.contains(marker));
            let placeholder = nested.as_str().is_some_and(|text| {
                matches!(
                    text,
                    "[REENTER_SECRET]" | "[REENTER_CONNECTION]" | "[REDACTED_SECRET]"
                )
            });
            (sensitive && !nested.is_null() && !placeholder)
                || nested
                    .as_str()
                    .is_some_and(|text| url_contains_credentials(text))
                || contains_unredacted_connection_secret(nested)
        }),
        Value::Array(items) => items.iter().any(contains_unredacted_connection_secret),
        Value::String(text) => url_contains_credentials(text),
        _ => false,
    }
}

fn imported_name(original: &str, new_id: &str) -> String {
    let suffix = format!(" (imported {})", &new_id[..8.min(new_id.len())]);
    let mut end = original.len().min(128usize.saturating_sub(suffix.len()));
    while !original.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{}", original[..end].trim_end(), suffix)
}

async fn preview_bundle(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    body: Bytes,
) -> Response {
    let bundle = match parse_bundle(&body) {
        Ok(bundle) => bundle,
        Err(message) => return api_error(StatusCode::UNPROCESSABLE_ENTITY, &message),
    };
    let existing = match sqlx::query_scalar::<_, String>(
        "SELECT name FROM assets WHERE workspace_id = ? AND archived_at IS NULL
         UNION ALL SELECT name FROM connection_profiles WHERE workspace_id = ?",
    )
    .bind(&user.workspace_id)
    .bind(&user.workspace_id)
    .fetch_all(&state.db)
    .await
    {
        Ok(names) => names.into_iter().collect::<HashSet<_>>(),
        Err(_) => {
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Could not check workspace conflicts",
            )
        }
    };
    let mut conflicts = bundle
        .assets
        .iter()
        .filter_map(|asset| {
            let name = asset.get("name")?.as_str()?;
            existing.contains(name).then(|| name.to_string())
        })
        .collect::<Vec<_>>();
    conflicts.extend(bundle.connections.iter().filter_map(|connection| {
        let name = connection.get("name")?.as_str()?;
        existing.contains(name).then(|| name.to_string())
    }));
    let asset_summary = bundle
        .assets
        .iter()
        .map(|asset| json!({ "name": asset["name"], "kind": asset["kind"], "revisions": asset["revisions"].as_array().map_or(0, Vec::len) }))
        .collect::<Vec<_>>();
    let connection_summary = bundle
        .connections
        .iter()
        .map(|connection| json!({ "name": connection["name"], "connector_type": connection["connector_type"], "secrets_need_reentry": connection_needs_reentry(connection) }))
        .collect::<Vec<_>>();
    Json(json!({
        "format_version": BUNDLE_FORMAT_VERSION,
        "assets": asset_summary,
        "asset_count": bundle.assets.len(),
        "revision_count": bundle.revision_count,
        "connections": connection_summary,
        "connection_count": bundle.connections.len(),
        "conflicts": conflicts,
        "import_mode": "merge_as_new"
    }))
    .into_response()
}

async fn import_bundle(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    body: Bytes,
) -> Response {
    let bundle = match parse_bundle(&body) {
        Ok(bundle) => bundle,
        Err(message) => return api_error(StatusCode::UNPROCESSABLE_ENTITY, &message),
    };
    let now = Utc::now().to_rfc3339();
    let mut id_map = HashMap::<String, String>::new();
    for asset in &bundle.assets {
        id_map.insert(
            asset["id"].as_str().unwrap_or_default().to_string(),
            Uuid::new_v4().to_string(),
        );
        for revision in asset["revisions"].as_array().into_iter().flatten() {
            id_map.insert(
                revision["id"].as_str().unwrap_or_default().to_string(),
                Uuid::new_v4().to_string(),
            );
        }
    }
    for connection in &bundle.connections {
        id_map.insert(
            connection["id"].as_str().unwrap_or_default().to_string(),
            Uuid::new_v4().to_string(),
        );
    }

    let mut imported_assets = Vec::with_capacity(bundle.assets.len());
    for asset in &bundle.assets {
        let old_id = asset["id"].as_str().unwrap_or_default();
        let new_id = id_map.get(old_id).cloned().ok_or("Invalid asset identity");
        let Ok(new_id) = new_id else {
            return api_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "A bundle asset is missing its identity",
            );
        };
        let mut revisions = Vec::new();
        for revision in asset["revisions"].as_array().into_iter().flatten() {
            let old_revision_id = revision["id"].as_str().unwrap_or_default();
            let Some(new_revision_id) = id_map.get(old_revision_id).cloned() else {
                return api_error(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "A bundle revision is missing its identity",
                );
            };
            let mut definition = revision["definition"].clone();
            rewrite_ids(&mut definition, &id_map);
            if let Err(message) = validate_imported_definition(
                asset["kind"].as_str().unwrap_or_default(),
                &definition,
                &id_map,
            ) {
                return api_error(StatusCode::UNPROCESSABLE_ENTITY, &message);
            }
            let definition_json = definition.to_string();
            revisions.push(json!({
                "id": new_revision_id,
                "version": revision["version"],
                "change_note": revision["change_note"],
                "definition": definition,
                "definition_json": definition_json,
                "checksum": compute_sha256(definition_json.as_bytes())
            }));
        }
        if revisions.is_empty() {
            return api_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "Every bundled asset must include at least one published revision",
            );
        }
        let name = imported_name(asset["name"].as_str().unwrap_or("Imported asset"), &new_id);
        imported_assets.push(json!({
            "id": new_id,
            "old_id": old_id,
            "kind": asset["kind"],
            "name": name,
            "description": asset["description"],
            "revisions": revisions
        }));
    }

    let mut tx = match state.db.begin().await {
        Ok(tx) => tx,
        Err(_) => {
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Import service is temporarily unavailable",
            )
        }
    };
    for asset in &imported_assets {
        let latest = asset["revisions"]
            .as_array()
            .and_then(|items| items.last())
            .expect("revisions were validated");
        if sqlx::query(
            "INSERT INTO assets (id, workspace_id, kind, name, description, draft_json, draft_version, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(asset["id"].as_str().unwrap_or_default())
        .bind(&user.workspace_id)
        .bind(asset["kind"].as_str().unwrap_or_default())
        .bind(asset["name"].as_str().unwrap_or_default())
        .bind(asset["description"].as_str().unwrap_or_default())
        .bind(latest["definition_json"].as_str().unwrap_or("{}"))
        .bind(latest["version"].as_i64().unwrap_or(1))
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .is_err()
        {
            let _ = tx.rollback().await;
            return api_error(StatusCode::CONFLICT, "Could not add an imported asset; no bundle changes were kept");
        }
        for revision in asset["revisions"].as_array().into_iter().flatten() {
            if sqlx::query(
                "INSERT INTO asset_revisions (id, asset_id, version, definition_json, checksum, change_note, author_id, created_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(revision["id"].as_str().unwrap_or_default())
            .bind(asset["id"].as_str().unwrap_or_default())
            .bind(revision["version"].as_i64().unwrap_or(1))
            .bind(revision["definition_json"].as_str().unwrap_or("{}"))
            .bind(revision["checksum"].as_str().unwrap_or_default())
            .bind(revision["change_note"].as_str().unwrap_or("Imported revision"))
            .bind(&user.id)
            .bind(&now)
            .execute(&mut *tx)
            .await
            .is_err()
            {
                let _ = tx.rollback().await;
                return api_error(StatusCode::CONFLICT, "Could not add an imported revision; no bundle changes were kept");
            }
        }
    }

    let mut imported_connections = 0usize;
    let secret_reentry_required = bundle.connections.iter().any(connection_needs_reentry);
    for connection in &bundle.connections {
        let old_id = connection["id"].as_str().unwrap_or_default();
        let Some(new_id) = id_map.get(old_id) else {
            continue;
        };
        let mut settings = connection["settings"].clone();
        rewrite_ids(&mut settings, &id_map);
        let refs = connection["secret_refs"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        let unresolved = refs
            .keys()
            .map(|key| (key.clone(), json!("[REENTER_SECRET]")))
            .collect::<serde_json::Map<_, _>>();
        if sqlx::query(
            "INSERT INTO connection_profiles (id, workspace_id, name, connector_type, settings_json, secret_refs_json, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(new_id)
        .bind(&user.workspace_id)
        .bind(imported_name(connection["name"].as_str().unwrap_or("Imported connection"), new_id))
        .bind(connection["connector_type"].as_str().unwrap_or_default())
        .bind(settings.to_string())
        .bind(Value::Object(unresolved).to_string())
        .bind(&now)
        .execute(&mut *tx)
        .await
        .is_err()
        {
            let _ = tx.rollback().await;
            return api_error(StatusCode::CONFLICT, "Could not add an imported connection; no bundle changes were kept");
        }
        imported_connections += 1;
    }

    for asset in &imported_assets {
        if asset["kind"].as_str() != Some("suite") {
            continue;
        }
        for revision in asset["revisions"].as_array().into_iter().flatten() {
            let suite_definition = &revision["definition"];
            for (index, case_ref) in suite_definition
                .get("cases")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
            {
                let Some(dependency_id) = case_ref.get("revision_id").and_then(Value::as_str)
                else {
                    continue;
                };
                if sqlx::query(
                    "INSERT INTO asset_dependencies (revision_id, dependency_revision_id, alias) VALUES (?, ?, ?)",
                )
                .bind(revision["id"].as_str().unwrap_or_default())
                .bind(dependency_id)
                .bind(format!("case_{}", index + 1))
                .execute(&mut *tx)
                    .await
                    .is_err()
                {
                    let _ = tx.rollback().await;
                    return api_error(StatusCode::CONFLICT, "Could not link an imported suite case; no bundle changes were kept");
                }
            }
        }
    }
    let audit = sqlx::query(
        "INSERT INTO audit_events (id, actor_id, action, target_type, target_id, changes_json, created_at)
         VALUES (?, ?, 'bundle.import', 'workspace', ?, ?, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(&user.id)
    .bind(&user.workspace_id)
    .bind(json!({ "asset_count": imported_assets.len(), "connection_count": imported_connections, "format_version": BUNDLE_FORMAT_VERSION }).to_string())
    .bind(&now)
    .execute(&mut *tx)
    .await;
    if audit.is_err() || tx.commit().await.is_err() {
        return api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not complete the bundle import transaction",
        );
    }
    Json(json!({
        "status": "imported",
        "asset_count": imported_assets.len(),
        "revision_count": bundle.revision_count,
        "connection_count": imported_connections,
        "secret_reentry_required": secret_reentry_required,
        "mode": "merge_as_new"
    }))
    .into_response()
}

struct ParsedBundle {
    assets: Vec<Value>,
    connections: Vec<Value>,
    revision_count: usize,
}

fn parse_bundle(bytes: &[u8]) -> Result<ParsedBundle, String> {
    if bytes.is_empty() || bytes.len() > MAX_ARCHIVE_BYTES {
        return Err("Bundle must be a non-empty ZIP file smaller than 10 MiB".to_string());
    }
    let files = read_stored_zip(bytes)?;
    let manifest: Value = serde_json::from_slice(required_file(&files, "manifest.json")?)
        .map_err(|_| "Bundle manifest is invalid JSON".to_string())?;
    if manifest.get("format_version").and_then(Value::as_u64) != Some(BUNDLE_FORMAT_VERSION) {
        return Err("This bundle format version is not supported".to_string());
    }
    let assets_bytes = required_file(&files, "definitions/assets.json")?;
    let connections_bytes = required_file(&files, "definitions/connections.json")?;
    let hashes = manifest
        .get("files")
        .and_then(Value::as_object)
        .ok_or_else(|| "Bundle manifest is missing its file checksums".to_string())?;
    for (path, bytes) in [
        ("definitions/assets.json", assets_bytes),
        ("definitions/connections.json", connections_bytes),
    ] {
        if hashes.get(path).and_then(Value::as_str) != Some(compute_sha256(bytes).as_str()) {
            return Err(format!("Bundle checksum does not match {}", path));
        }
    }
    if hashes.len() != 2 {
        return Err("Bundle manifest counts or file list do not match its contents".to_string());
    }
    verify_checksum_file(
        required_file(&files, "checksums.sha256")?,
        assets_bytes,
        connections_bytes,
    )?;

    let assets_doc: Value = serde_json::from_slice(assets_bytes)
        .map_err(|_| "Bundle asset definitions are invalid JSON".to_string())?;
    let connections_doc: Value = serde_json::from_slice(connections_bytes)
        .map_err(|_| "Bundle connection definitions are invalid JSON".to_string())?;
    let assets = assets_doc
        .get("assets")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| "Bundle must contain an assets array".to_string())?;
    let connections = connections_doc
        .get("connections")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| "Bundle must contain a connections array".to_string())?;
    if assets.len() > 1000 || connections.len() > 200 {
        return Err("Bundle contains too many assets or connection profiles".to_string());
    }
    let total = assets_bytes.len()
        + connections_bytes.len()
        + required_file(&files, "manifest.json")?.len();
    if total > MAX_EXPANDED_BYTES {
        return Err("Bundle expands beyond the 32 MiB limit".to_string());
    }
    let revision_count = assets
        .iter()
        .map(|asset| {
            asset
                .get("revisions")
                .and_then(Value::as_array)
                .map_or(0, Vec::len)
        })
        .sum::<usize>();
    if manifest.get("asset_count").and_then(Value::as_u64) != Some(assets.len() as u64)
        || manifest.get("revision_count").and_then(Value::as_u64) != Some(revision_count as u64)
    {
        return Err("Bundle manifest counts do not match its contents".to_string());
    }
    if revision_count > 10_000 {
        return Err("Bundle contains more than 10000 revisions".to_string());
    }
    let mut identities = HashSet::new();
    for asset in &assets {
        let id = asset
            .get("id")
            .and_then(Value::as_str)
            .ok_or("Bundle asset is missing its ID")?;
        Uuid::parse_str(id).map_err(|_| "Bundle asset ID is invalid".to_string())?;
        if !identities.insert(id.to_string()) {
            return Err("Bundle contains duplicate asset IDs".to_string());
        }
        if !matches!(
            asset.get("kind").and_then(Value::as_str),
            Some("suite" | "case")
        ) {
            return Err("Bundle contains an unsupported asset kind".to_string());
        }
        if asset
            .get("name")
            .and_then(Value::as_str)
            .is_none_or(|name| name.trim().is_empty() || name.len() > 128)
        {
            return Err("Bundle asset name is invalid".to_string());
        }
        if asset
            .get("description")
            .is_some_and(|value| !value.is_string() && !value.is_null())
        {
            return Err("Bundle asset description must be text".to_string());
        }
        let revisions = asset
            .get("revisions")
            .and_then(Value::as_array)
            .ok_or("Bundle asset has no revisions")?;
        if revisions.is_empty() {
            return Err("Every bundled asset must include a published revision".to_string());
        }
        let mut previous_version = 0i64;
        for revision in revisions {
            let revision_id = revision
                .get("id")
                .and_then(Value::as_str)
                .ok_or("Bundle revision is missing its ID")?;
            Uuid::parse_str(revision_id)
                .map_err(|_| "Bundle revision ID is invalid".to_string())?;
            if !identities.insert(revision_id.to_string()) {
                return Err("Bundle contains duplicate IDs".to_string());
            }
            let version = revision
                .get("version")
                .and_then(Value::as_i64)
                .ok_or("Bundle revision version is invalid")?;
            if version <= previous_version {
                return Err("Bundle revision version is invalid".to_string());
            }
            previous_version = version;
            if !revision.get("definition").is_some_and(Value::is_object) {
                return Err("Bundle revision definition must be an object".to_string());
            }
        }
    }
    for connection in &connections {
        let id = connection
            .get("id")
            .and_then(Value::as_str)
            .ok_or("Bundle connection is missing its ID")?;
        Uuid::parse_str(id).map_err(|_| "Bundle connection ID is invalid".to_string())?;
        if !identities.insert(id.to_string()) {
            return Err("Bundle contains duplicate IDs".to_string());
        }
        if !matches!(
            connection.get("connector_type").and_then(Value::as_str),
            Some("mysql" | "cassandra" | "mongodb" | "api" | "http" | "parquet" | "delta")
        ) {
            return Err("Bundle contains an unsupported connection type".to_string());
        }
        if connection
            .get("name")
            .and_then(Value::as_str)
            .is_none_or(|name| name.trim().is_empty() || name.chars().count() > 128)
        {
            return Err("Bundle connection name is invalid".to_string());
        }
        if !connection.get("settings").is_some_and(Value::is_object)
            || !connection.get("secret_refs").is_some_and(Value::is_object)
        {
            return Err(
                "Bundle connection settings and secret references must be objects".to_string(),
            );
        }
        let settings = &connection["settings"];
        let secret_refs = connection["secret_refs"]
            .as_object()
            .expect("validated object");
        if contains_unredacted_connection_secret(settings)
            || secret_refs.values().any(|value| {
                value
                    .as_str()
                    .is_none_or(|reference| reference.trim().is_empty())
            })
        {
            return Err(
                "Bundle connection contains inline credentials or invalid secret references"
                    .to_string(),
            );
        }
    }
    validate_bundled_dependencies(&assets)?;
    Ok(ParsedBundle {
        assets,
        connections,
        revision_count,
    })
}

fn validate_bundled_dependencies(assets: &[Value]) -> Result<(), String> {
    let revisions = assets
        .iter()
        .flat_map(|asset| {
            asset
                .get("revisions")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(move |revision| (asset, revision))
        })
        .map(|(asset, revision)| {
            (
                revision["id"].as_str().unwrap_or_default().to_string(),
                (asset, revision),
            )
        })
        .collect::<HashMap<_, _>>();
    for asset in assets {
        for revision in asset["revisions"].as_array().into_iter().flatten() {
            let definition = &revision["definition"];
            crate::variables::validate_variable_definitions(definition.get("variables"))?;
            if asset["kind"].as_str() == Some("case") {
                crate::orchestrator::validate_case_data_set(definition)?;
                validate_bundled_case_nodes(definition)?;
                crate::api::assets::validate_case_variable_references(definition, None)?;
            } else {
                crate::api::assets::validate_suite_hook_nodes(definition)?;
                crate::resource_locks::validate_suite_resource_locks(definition)?;
                crate::variables::validate_input_schema(definition.get("input_schema"))?;
                let cases = definition
                    .get("cases")
                    .and_then(Value::as_array)
                    .ok_or("Bundled suite has no cases array")?;
                if cases.is_empty() {
                    return Err("Bundled suite cannot have an empty case list".to_string());
                }
                for case_ref in cases {
                    let case_id = case_ref
                        .get("case_id")
                        .and_then(Value::as_str)
                        .ok_or("Bundled suite case reference is missing an asset ID")?;
                    let dep_id = case_ref
                        .get("revision_id")
                        .and_then(Value::as_str)
                        .ok_or("Bundled suite case reference is missing a revision ID")?;
                    let dep = revisions
                        .get(dep_id)
                        .ok_or("Bundle suite refers to a case revision that is not included")?;
                    if dep.0["kind"].as_str() != Some("case") {
                        return Err(
                            "Bundle suite can only reference included case revisions".to_string()
                        );
                    }
                    if dep.0["id"].as_str() != Some(case_id) {
                        return Err(
                            "Bundle suite case asset and revision IDs do not match".to_string()
                        );
                    }
                    crate::api::assets::validate_case_variable_references(
                        &dep.1["definition"],
                        Some(definition),
                    )?;
                }
            }
        }
    }
    Ok(())
}

fn validate_bundled_case_nodes(definition: &Value) -> Result<(), String> {
    let nodes = definition
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or("Bundled case has no nodes array")?;
    let edges = definition
        .get("edges")
        .and_then(Value::as_array)
        .ok_or("Bundled case has no edges array")?;
    if nodes.is_empty() {
        return Err("Bundled case cannot have an empty workflow".to_string());
    }
    let mut node_ids = HashSet::new();
    for node in nodes {
        let id = node.get("id").and_then(Value::as_str).unwrap_or_default();
        let name = node.get("name").and_then(Value::as_str).unwrap_or_default();
        let timeout = node
            .get("timeout_seconds")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        if id.is_empty() || !node_ids.insert(id.to_string()) {
            return Err("Every bundled workflow node needs a unique non-empty ID".to_string());
        }
        if name.trim().is_empty()
            || name.len() > 128
            || !(1..=3600).contains(&timeout)
            || node.get("type_version").and_then(Value::as_i64) != Some(1)
        {
            return Err("A bundled workflow node has an invalid name or timeout".to_string());
        }
        if !matches!(
            node.get("type").and_then(Value::as_str),
            Some(
                "api.request"
                    | "wait.until"
                    | "db.mysql"
                    | "db.cassandra"
                    | "db.mongodb"
                    | "data.tabular"
                    | "sleep.wait"
            )
        ) {
            return Err("Bundle contains an unsupported workflow node".to_string());
        }
        if node.get("type").and_then(Value::as_str) == Some("wait.until") {
            let config = node.get("config").unwrap_or(&Value::Null);
            let target = config
                .get("target")
                .and_then(Value::as_str)
                .unwrap_or("api");
            let method = config
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or("GET")
                .to_ascii_uppercase();
            let interval = config
                .get("poll_interval_seconds")
                .and_then(Value::as_i64)
                .unwrap_or(2);
            let valid_target = match target {
                "api" => matches!(method.as_str(), "GET" | "HEAD" | "OPTIONS"),
                "mysql" => config
                    .get("query")
                    .and_then(Value::as_str)
                    .and_then(|query| query.split_whitespace().next())
                    .is_some_and(|word| word.eq_ignore_ascii_case("SELECT")),
                "cassandra" => config
                    .get("query")
                    .and_then(Value::as_str)
                    .and_then(|query| query.split_whitespace().next())
                    .is_some_and(|word| word.eq_ignore_ascii_case("SELECT")),
                "mongodb" => config
                    .get("collection")
                    .and_then(Value::as_str)
                    .is_some_and(|collection| !collection.trim().is_empty()),
                _ => false,
            };
            if !valid_target || !(1..=60).contains(&interval) {
                return Err("Bundled wait-until node has an invalid or unsafe target".to_string());
            }
        }
        if !matches!(
            node.get("phase").and_then(Value::as_str).unwrap_or("main"),
            "setup" | "main" | "cleanup"
        ) {
            return Err("Bundle contains an unsupported workflow phase".to_string());
        }
    }
    if edges.len() != nodes.len().saturating_sub(1)
        || nodes.windows(2).any(|pair| {
            let source = pair[0].get("id").and_then(Value::as_str);
            let target = pair[1].get("id").and_then(Value::as_str);
            !edges.iter().any(|edge| {
                edge.get("source").and_then(Value::as_str) == source
                    && edge.get("target").and_then(Value::as_str) == target
            })
        })
    {
        return Err("Bundle contains unsupported workflow routing".to_string());
    }
    Ok(())
}

fn validate_imported_definition(
    kind: &str,
    definition: &Value,
    id_map: &HashMap<String, String>,
) -> Result<(), String> {
    if kind == "case" {
        crate::variables::validate_variable_definitions(definition.get("variables"))?;
        crate::orchestrator::validate_case_data_set(definition)?;
        validate_bundled_case_nodes(definition)?;
    } else if kind == "suite" {
        crate::variables::validate_variable_definitions(definition.get("variables"))?;
        crate::variables::validate_input_schema(definition.get("input_schema"))?;
        let cases = definition
            .get("cases")
            .and_then(Value::as_array)
            .ok_or("Imported suite has no cases")?;
        for case_ref in cases {
            let case_id = case_ref
                .get("case_id")
                .and_then(Value::as_str)
                .ok_or("Imported suite case is missing its asset ID")?;
            let revision_id = case_ref
                .get("revision_id")
                .and_then(Value::as_str)
                .ok_or("Imported suite case is missing its revision ID")?;
            if !id_map.contains_key(case_id) || !id_map.contains_key(revision_id) {
                return Err(
                    "Imported suite refers to a case not included in the bundle".to_string()
                );
            }
        }
    } else {
        return Err("Imported asset type is not supported".to_string());
    }
    Ok(())
}

fn rewrite_ids(value: &mut Value, id_map: &HashMap<String, String>) {
    match value {
        Value::String(text) => {
            if let Some(replacement) = id_map.get(text) {
                *text = replacement.clone();
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| rewrite_ids(item, id_map)),
        Value::Object(items) => items
            .values_mut()
            .for_each(|item| rewrite_ids(item, id_map)),
        _ => {}
    }
}

fn scrub_sensitive_values(value: &mut Value) {
    match value {
        Value::Object(items) => {
            for (key, nested) in items.iter_mut() {
                let lowered = key.to_ascii_lowercase();
                let sensitive = [
                    "password",
                    "secret",
                    "token",
                    "authorization",
                    "cookie",
                    "credential",
                    "api_key",
                    "apikey",
                ]
                .iter()
                .any(|marker| lowered.contains(marker));
                if sensitive {
                    *nested = json!("[REENTER_SECRET]");
                } else {
                    scrub_sensitive_values(nested);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(scrub_sensitive_values),
        Value::String(text) if url_contains_credentials(text) => {
            *text = "[REENTER_CONNECTION]".to_string();
        }
        _ => {}
    }
}

fn scrub_connection_settings(value: &mut Value) {
    match value {
        Value::Object(items) => {
            let keys = items.keys().cloned().collect::<Vec<_>>();
            for key in keys {
                let lowered = key.to_ascii_lowercase();
                let sensitive = [
                    "password",
                    "secret",
                    "token",
                    "authorization",
                    "cookie",
                    "credential",
                    "api_key",
                    "apikey",
                ]
                .iter()
                .any(|marker| lowered.contains(marker));
                let is_reference = lowered.ends_with("_secret")
                    || lowered == "secret_ref"
                    || lowered == "secret_refs";
                if sensitive {
                    let target_key = if is_reference {
                        key.clone()
                    } else {
                        format!("{}_secret", key)
                    };
                    items.remove(&key);
                    items.insert(target_key, json!("[REENTER_SECRET]"));
                } else if items
                    .get(&key)
                    .and_then(Value::as_str)
                    .is_some_and(url_contains_credentials)
                {
                    items.insert(key, json!("[REENTER_CONNECTION]"));
                } else if let Some(nested) = items.get_mut(&key) {
                    scrub_connection_settings(nested);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(scrub_connection_settings),
        Value::String(text) if url_contains_credentials(text) => {
            *text = "[REENTER_CONNECTION]".to_string();
        }
        _ => {}
    }
}

fn url_contains_credentials(value: &str) -> bool {
    let Some((_, rest)) = value.split_once("://") else {
        return false;
    };
    let authority = rest
        .split(|character| matches!(character, '/' | '?' | '#'))
        .next()
        .unwrap_or_default();
    if authority.contains('@') {
        return true;
    }
    let sensitive_parameter = |key: &str| {
        let key = key.to_ascii_lowercase();
        [
            "password",
            "passwd",
            "secret",
            "token",
            "key",
            "credential",
            "authorization",
            "cookie",
            "api_key",
            "apikey",
        ]
        .iter()
        .any(|marker| key.contains(marker))
    };
    rest.split_once('?')
        .map(|(_, query)| {
            query
                .split(|character| matches!(character, '&' | '#'))
                .any(|part| {
                    part.split_once('=')
                        .is_some_and(|(key, value)| sensitive_parameter(key) && !value.is_empty())
                })
        })
        .unwrap_or(false)
}

fn redact_known_secrets(value: &mut Value, known_secrets: &[String]) {
    match value {
        Value::String(text) => {
            for secret in known_secrets {
                if text.contains(secret) {
                    *text = text.replace(secret, "[REDACTED_SECRET]");
                }
            }
        }
        Value::Array(items) => items
            .iter_mut()
            .for_each(|item| redact_known_secrets(item, known_secrets)),
        Value::Object(items) => items
            .values_mut()
            .for_each(|item| redact_known_secrets(item, known_secrets)),
        _ => {}
    }
}

fn verify_checksum_file(
    checksum_file: &[u8],
    assets: &[u8],
    connections: &[u8],
) -> Result<(), String> {
    let text = std::str::from_utf8(checksum_file)
        .map_err(|_| "Bundle checksum file is not UTF-8".to_string())?;
    let mut entries = HashMap::new();
    for line in text.lines() {
        let Some((digest, name)) = line.split_once("  ") else {
            return Err("Bundle checksum file is malformed".to_string());
        };
        if digest.len() != 64
            || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
            || !matches!(
                name,
                "definitions/assets.json" | "definitions/connections.json"
            )
            || entries.insert(name, digest).is_some()
        {
            return Err("Bundle checksum file contains an invalid or duplicate entry".to_string());
        }
    }
    if entries.len() != 2 {
        return Err("Bundle checksum file must list exactly two definition files".to_string());
    }
    for (name, bytes) in [
        ("definitions/assets.json", assets),
        ("definitions/connections.json", connections),
    ] {
        if entries.get(name).copied() != Some(compute_sha256(bytes).as_str()) {
            return Err(format!("Bundle checksum list does not match {}", name));
        }
    }
    Ok(())
}

fn required_file<'a>(files: &'a HashMap<String, Vec<u8>>, name: &str) -> Result<&'a [u8], String> {
    files
        .get(name)
        .map(Vec::as_slice)
        .ok_or_else(|| format!("Bundle is missing {}", name))
}

fn api_error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

fn write_stored_zip(files: HashMap<String, Vec<u8>>) -> Result<Vec<u8>, String> {
    if files.len() > MAX_ARCHIVE_FILES {
        return Err("Too many bundle files".to_string());
    }
    let mut output = Vec::new();
    let mut central = Vec::new();
    let mut names = files.into_iter().collect::<Vec<_>>();
    names.sort_by(|left, right| left.0.cmp(&right.0));
    for (name, bytes) in names {
        if name.contains('/')
            && !matches!(
                name.as_str(),
                "definitions/assets.json" | "definitions/connections.json"
            )
        {
            return Err("Invalid bundle path".to_string());
        }
        let name_bytes = name.as_bytes();
        let name_len = u16::try_from(name_bytes.len())
            .map_err(|_| "Bundle file name is too long".to_string())?;
        let size =
            u32::try_from(bytes.len()).map_err(|_| "Bundle file is too large".to_string())?;
        let crc = crc32(&bytes);
        let local_offset =
            u32::try_from(output.len()).map_err(|_| "Bundle is too large".to_string())?;
        push_u32(&mut output, 0x0403_4b50);
        push_u16(&mut output, 20);
        push_u16(&mut output, 0);
        push_u16(&mut output, 0);
        push_u16(&mut output, 0);
        push_u16(&mut output, 0);
        push_u32(&mut output, crc);
        push_u32(&mut output, size);
        push_u32(&mut output, size);
        push_u16(&mut output, name_len);
        push_u16(&mut output, 0);
        output.extend_from_slice(name_bytes);
        output.extend_from_slice(&bytes);

        push_u32(&mut central, 0x0201_4b50);
        push_u16(&mut central, 20);
        push_u16(&mut central, 20);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u32(&mut central, crc);
        push_u32(&mut central, size);
        push_u32(&mut central, size);
        push_u16(&mut central, name_len);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u32(&mut central, 0);
        push_u32(&mut central, local_offset);
        central.extend_from_slice(name_bytes);
    }
    let central_offset =
        u32::try_from(output.len()).map_err(|_| "Bundle is too large".to_string())?;
    let central_size =
        u32::try_from(central.len()).map_err(|_| "Bundle is too large".to_string())?;
    output.extend_from_slice(&central);
    let entry_count = count_from_central(&central)?;
    push_u32(&mut output, 0x0605_4b50);
    push_u16(&mut output, 0);
    push_u16(&mut output, 0);
    push_u16(&mut output, entry_count);
    push_u16(&mut output, entry_count);
    push_u32(&mut output, central_size);
    push_u32(&mut output, central_offset);
    push_u16(&mut output, 0);
    Ok(output)
}

fn count_from_central(bytes: &[u8]) -> Result<u16, String> {
    let mut cursor = 0usize;
    let mut count = 0u16;
    while cursor < bytes.len() {
        if read_u32(bytes, cursor)? != 0x0201_4b50 {
            return Err("Invalid central directory".to_string());
        }
        let name = read_u16(bytes, cursor + 28)? as usize;
        let extra = read_u16(bytes, cursor + 30)? as usize;
        let comment = read_u16(bytes, cursor + 32)? as usize;
        cursor = cursor
            .checked_add(46 + name + extra + comment)
            .ok_or("Invalid ZIP size")?;
        count = count.checked_add(1).ok_or("Too many bundle files")?;
    }
    Ok(count)
}

fn read_stored_zip(bytes: &[u8]) -> Result<HashMap<String, Vec<u8>>, String> {
    if bytes.len() < 22 {
        return Err("Bundle is not a valid ZIP archive".to_string());
    }
    let eocd_offset = (0..=bytes.len() - 22)
        .rev()
        .take(65_558)
        .find(|offset| read_u32(bytes, *offset).ok() == Some(0x0605_4b50))
        .ok_or("ZIP end record is missing")?;
    if eocd_offset + 22 != bytes.len() {
        return Err("ZIP comments and trailing data are not supported".to_string());
    }
    if read_u16(bytes, eocd_offset + 4)? != 0 || read_u16(bytes, eocd_offset + 6)? != 0 {
        return Err("Multi-disk ZIP archives are not supported".to_string());
    }
    let entry_count = read_u16(bytes, eocd_offset + 10)? as usize;
    if read_u16(bytes, eocd_offset + 8)? as usize != entry_count || entry_count > MAX_ARCHIVE_FILES
    {
        return Err("ZIP contains too many files or inconsistent entry counts".to_string());
    }
    let central_size = read_u32(bytes, eocd_offset + 12)? as usize;
    let central_offset = read_u32(bytes, eocd_offset + 16)? as usize;
    if central_offset.checked_add(central_size) != Some(eocd_offset) {
        return Err("ZIP central directory bounds are invalid".to_string());
    }

    let mut files = HashMap::new();
    let mut local_meta = HashMap::<String, (u32, u32, u32, usize)>::new();
    let mut cursor = 0usize;
    let mut expanded = 0usize;
    while cursor < central_offset {
        if read_u32(bytes, cursor)? != 0x0403_4b50 {
            return Err("ZIP local file header is invalid".to_string());
        }
        let flags = read_u16(bytes, cursor + 6)?;
        let method = read_u16(bytes, cursor + 8)?;
        if flags != 0 || method != 0 {
            return Err(
                "Only uncompressed, non-encrypted TestIT bundles are supported".to_string(),
            );
        }
        let crc = read_u32(bytes, cursor + 14)?;
        let compressed = read_u32(bytes, cursor + 18)?;
        let size = read_u32(bytes, cursor + 22)?;
        let name_len = read_u16(bytes, cursor + 26)? as usize;
        let extra_len = read_u16(bytes, cursor + 28)? as usize;
        if compressed != size {
            return Err("ZIP file sizes are inconsistent".to_string());
        }
        let name_start = cursor.checked_add(30).ok_or("Invalid ZIP offset")?;
        let data_start = name_start
            .checked_add(name_len + extra_len)
            .ok_or("Invalid ZIP offset")?;
        let data_end = data_start
            .checked_add(size as usize)
            .ok_or("Invalid ZIP offset")?;
        if data_end > central_offset {
            return Err("ZIP entry exceeds archive bounds".to_string());
        }
        let name = std::str::from_utf8(
            bytes
                .get(name_start..name_start + name_len)
                .ok_or("Invalid ZIP name")?,
        )
        .map_err(|_| "ZIP file path is not UTF-8")?
        .to_string();
        if !matches!(
            name.as_str(),
            "manifest.json"
                | "checksums.sha256"
                | "definitions/assets.json"
                | "definitions/connections.json"
        ) || name.starts_with('/')
            || name.contains("..")
            || name.contains('\\')
        {
            return Err("Bundle contains an unsupported or unsafe file path".to_string());
        }
        if files.contains_key(&name) {
            return Err("Bundle contains duplicate file paths".to_string());
        }
        let data = bytes
            .get(data_start..data_end)
            .ok_or("ZIP entry data is incomplete")?
            .to_vec();
        if crc32(&data) != crc {
            return Err(format!("ZIP entry checksum failed for {}", name));
        }
        expanded = expanded
            .checked_add(data.len())
            .ok_or("Bundle expanded size overflow")?;
        if expanded > MAX_EXPANDED_BYTES {
            return Err("Bundle expands beyond the 32 MiB limit".to_string());
        }
        local_meta.insert(name.clone(), (crc, compressed, size, cursor));
        files.insert(name, data);
        cursor = data_end;
    }
    if cursor != central_offset || files.len() != entry_count {
        return Err("ZIP local and central directories do not match".to_string());
    }
    let mut central_cursor = central_offset;
    let mut central_names = HashSet::new();
    for _ in 0..entry_count {
        if read_u32(bytes, central_cursor)? != 0x0201_4b50 {
            return Err("ZIP central directory entry is invalid".to_string());
        }
        let flags = read_u16(bytes, central_cursor + 8)?;
        let method = read_u16(bytes, central_cursor + 10)?;
        let crc = read_u32(bytes, central_cursor + 16)?;
        let compressed = read_u32(bytes, central_cursor + 20)?;
        let size = read_u32(bytes, central_cursor + 24)?;
        let name_len = read_u16(bytes, central_cursor + 28)? as usize;
        let extra_len = read_u16(bytes, central_cursor + 30)? as usize;
        let comment_len = read_u16(bytes, central_cursor + 32)? as usize;
        let external_attrs = read_u32(bytes, central_cursor + 38)?;
        let local_offset = read_u32(bytes, central_cursor + 42)? as usize;
        if flags != 0 || method != 0 || external_attrs != 0 {
            return Err("ZIP central directory has unsupported flags or attributes".to_string());
        }
        let name_start = central_cursor + 46;
        let name = std::str::from_utf8(
            bytes
                .get(name_start..name_start + name_len)
                .ok_or("Invalid ZIP central directory name")?,
        )
        .map_err(|_| "ZIP path is not UTF-8")?;
        if !central_names.insert(name.to_string()) {
            return Err("ZIP central directory contains duplicate paths".to_string());
        }
        if local_meta.get(name) != Some(&(crc, compressed, size, local_offset)) {
            return Err("ZIP local and central directory entries do not match".to_string());
        }
        central_cursor = central_cursor
            .checked_add(46 + name_len + extra_len + comment_len)
            .ok_or("Invalid ZIP central directory size")?;
    }
    if central_cursor != eocd_offset || central_names.len() != files.len() {
        return Err("ZIP central directory size is inconsistent".to_string());
    }
    Ok(files)
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= *byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn push_u16(target: &mut Vec<u8>, value: u16) {
    target.extend_from_slice(&value.to_le_bytes());
}
fn push_u32(target: &mut Vec<u8>, value: u32) {
    target.extend_from_slice(&value.to_le_bytes());
}
fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, String> {
    Ok(u16::from_le_bytes(
        bytes
            .get(offset..offset + 2)
            .ok_or("ZIP is truncated")?
            .try_into()
            .map_err(|_| "ZIP is truncated")?,
    ))
}
fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..offset + 4)
            .ok_or("ZIP is truncated")?
            .try_into()
            .map_err(|_| "ZIP is truncated")?,
    ))
}
