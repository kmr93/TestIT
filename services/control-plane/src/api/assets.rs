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
    crypto::compute_sha256,
    models::{Asset, AssetRevision, PublishDraftRequest, PublishDraftResponse},
    AppState,
};

#[derive(Deserialize)]
pub struct CreateAssetRequest {
    pub kind: String,
    pub name: String,
    pub description: Option<String>,
    pub initial_draft: Option<Value>,
}

#[derive(Deserialize)]
pub struct UpdateDraftRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub draft_json: Value,
    pub expected_draft_version: i64,
}

pub async fn list_assets(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let assets = sqlx::query_as::<_, Asset>(
        "SELECT id, workspace_id, kind, name, description, '{}' AS draft_json, draft_version, archived_at, created_at, updated_at
         FROM assets WHERE workspace_id = ? AND archived_at IS NULL ORDER BY updated_at DESC"
    )
    .bind(&user.workspace_id)
    .fetch_all(&state.db)
    .await;

    match assets {
        Ok(list) => (StatusCode::OK, Json(json!(list))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

pub async fn create_asset(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(payload): Json<CreateAssetRequest>,
) -> impl IntoResponse {
    if !matches!(
        payload.kind.as_str(),
        "suite" | "case" | "template" | "script"
    ) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Unsupported asset kind" })),
        );
    }
    let id = Uuid::new_v4().to_string();
    let workspace_id = user.workspace_id;
    let draft_str = payload
        .initial_draft
        .map(|v| v.to_string())
        .unwrap_or_else(|| "{}".to_string());
    let desc = payload.description.unwrap_or_default();
    let now = Utc::now().to_rfc3339();

    let res = sqlx::query(
        "INSERT INTO assets (id, workspace_id, kind, name, description, draft_json, draft_version, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, 1, ?, ?)"
    )
    .bind(&id)
    .bind(&workspace_id)
    .bind(&payload.kind)
    .bind(&payload.name)
    .bind(&desc)
    .bind(&draft_str)
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await;

    match res {
        Ok(_) => (
            StatusCode::CREATED,
            Json(json!({
                "id": id,
                "kind": payload.kind,
                "name": payload.name,
                "description": desc,
                "draft_version": 1,
                "created_at": now
            })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

pub async fn get_draft(
    State(state): State<AppState>,
    Path(asset_id): Path<String>,
    Extension(user): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let asset = sqlx::query_as::<_, Asset>(
        "SELECT id, workspace_id, kind, name, description, draft_json, draft_version, archived_at, created_at, updated_at 
         FROM assets WHERE id = ? AND workspace_id = ?"
    )
    .bind(&asset_id)
    .bind(&user.workspace_id)
    .fetch_optional(&state.db)
    .await;

    match asset {
        Ok(Some(a)) => {
            let draft_parsed: Value = serde_json::from_str(&a.draft_json).unwrap_or(json!({}));
            (
                StatusCode::OK,
                Json(json!({
                    "id": a.id,
                    "kind": a.kind,
                    "name": a.name,
                    "description": a.description,
                    "draft_version": a.draft_version,
                    "draft": draft_parsed,
                    "updated_at": a.updated_at
                })),
            )
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Asset not found" })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

pub async fn update_draft(
    State(state): State<AppState>,
    Path(asset_id): Path<String>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(payload): Json<UpdateDraftRequest>,
) -> impl IntoResponse {
    let now = Utc::now().to_rfc3339();
    let draft_str = payload.draft_json.to_string();

    // Optimistic concurrency check on draft_version
    let res = sqlx::query(
        "UPDATE assets 
         SET draft_json = ?, draft_version = draft_version + 1, updated_at = ?,
             name = COALESCE(?, name), description = COALESCE(?, description)
         WHERE id = ? AND workspace_id = ? AND draft_version = ?",
    )
    .bind(&draft_str)
    .bind(&now)
    .bind(&payload.name)
    .bind(&payload.description)
    .bind(&asset_id)
    .bind(&user.workspace_id)
    .bind(payload.expected_draft_version)
    .execute(&state.db)
    .await;

    match res {
        Ok(result) => {
            if result.rows_affected() == 0 {
                (
                    StatusCode::CONFLICT,
                    Json(json!({
                        "error": "Conflict: draft has been modified by another operation",
                        "code": "STALE_DRAFT_VERSION"
                    })),
                )
            } else {
                (
                    StatusCode::OK,
                    Json(json!({
                        "id": asset_id,
                        "draft_version": payload.expected_draft_version + 1,
                        "updated_at": now
                    })),
                )
            }
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

pub async fn publish_revision(
    State(state): State<AppState>,
    Path(asset_id): Path<String>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(payload): Json<PublishDraftRequest>,
) -> impl IntoResponse {
    // 1. Fetch current asset draft
    let asset = match sqlx::query_as::<_, Asset>(
        "SELECT id, workspace_id, kind, name, description, draft_json, draft_version, archived_at, created_at, updated_at 
         FROM assets WHERE id = ? AND workspace_id = ?"
    )
    .bind(&asset_id)
    .bind(&user.workspace_id)
    .fetch_optional(&state.db)
    .await
    {
        Ok(Some(a)) => a,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "Asset not found" })),
            )
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        }
    };

    if asset.draft_version != payload.expected_draft_version {
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Expected draft version does not match active version",
                "active_version": asset.draft_version,
                "expected": payload.expected_draft_version
            })),
        );
    }

    // 2. Validate graph cycles if kind == "case"
    let draft_val: Value = match serde_json::from_str(&asset.draft_json) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("Invalid draft JSON: {}", e) })),
            )
        }
    };

    if let Err(message) =
        crate::variables::validate_variable_definitions(draft_val.get("variables"))
    {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": message, "code": "VARIABLE_DEFINITIONS_INVALID" })),
        );
    }

    if asset.kind == "case" {
        let nodes = draft_val.get("nodes").and_then(Value::as_array);
        let edges = draft_val.get("edges").and_then(Value::as_array);
        let (Some(nodes), Some(edges)) = (nodes, edges) else {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({ "error": "A case requires nodes and edges arrays" })),
            );
        };
        if nodes.is_empty() {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({ "error": "A case must contain at least one node" })),
            );
        }
        let mut node_ids = std::collections::HashSet::new();
        for node in nodes {
            let id = node.get("id").and_then(Value::as_str).unwrap_or_default();
            let node_type = node.get("type").and_then(Value::as_str).unwrap_or_default();
            let name = node.get("name").and_then(Value::as_str).unwrap_or_default();
            let timeout = node
                .get("timeout_seconds")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let type_version = node
                .get("type_version")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            if id.is_empty() || !node_ids.insert(id.to_string()) {
                return (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(json!({ "error": "Each node needs a unique non-empty id" })),
                );
            }
            if name.trim().is_empty()
                || name.len() > 128
                || !(1..=3600).contains(&timeout)
                || type_version != 1
            {
                return (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(
                        json!({ "error": format!("Node '{}' has an invalid name or timeout", id) }),
                    ),
                );
            }
            if !matches!(
                node_type,
                "api.request"
                    | "wait.until"
                    | "db.mysql"
                    | "db.mongodb"
                    | "data.tabular"
                    | "sleep.wait"
            ) {
                return (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(json!({ "error": format!("Node type '{}' is not supported", node_type) })),
                );
            }
        }
        for edge in edges {
            let source = edge
                .get("source")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let target = edge
                .get("target")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if source == target || !node_ids.contains(source) || !node_ids.contains(target) {
                return (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(
                        json!({ "error": "Every edge must connect two different nodes in this case" }),
                    ),
                );
            }
        }
        if let Err(cycle_err) = detect_cycles(nodes, edges) {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({
                    "error": format!("Workflow graph contains cycles: {}", cycle_err),
                    "code": "GRAPH_CYCLE_DETECTED"
                })),
            );
        }
        let sequential_edges_match = edges.len() == nodes.len().saturating_sub(1)
            && nodes.windows(2).all(|pair| {
                let source = pair[0].get("id").and_then(Value::as_str);
                let target = pair[1].get("id").and_then(Value::as_str);
                edges.iter().any(|edge| {
                    edge.get("source").and_then(Value::as_str) == source
                        && edge.get("target").and_then(Value::as_str) == target
                })
            });
        if !sequential_edges_match {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(
                    json!({ "error": "This editor currently publishes ordered sequences; graph branches and custom edge routing are not supported", "code": "GRAPH_ROUTING_UNSUPPORTED" }),
                ),
            );
        }
        for node in nodes {
            let phase = node.get("phase").and_then(Value::as_str).unwrap_or("main");
            if !matches!(phase, "setup" | "main" | "cleanup") {
                return (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(
                        json!({ "error": format!("Node phase '{}' is not supported", phase), "code": "NODE_PHASE_UNSUPPORTED" }),
                    ),
                );
            }
            if node.get("type").and_then(Value::as_str) == Some("wait.until") {
                let config = node.get("config").unwrap_or(&Value::Null);
                let method = config
                    .get("method")
                    .and_then(Value::as_str)
                    .unwrap_or("GET")
                    .to_ascii_uppercase();
                let poll_interval = config
                    .get("poll_interval_seconds")
                    .and_then(Value::as_i64)
                    .unwrap_or(2);
                if !matches!(method.as_str(), "GET" | "HEAD" | "OPTIONS")
                    || !(1..=60).contains(&poll_interval)
                {
                    return (
                        StatusCode::UNPROCESSABLE_ENTITY,
                        Json(
                            json!({ "error": "Wait-until requires an idempotent read method and a poll interval from 1 to 60 seconds", "code": "WAIT_UNTIL_CONFIG_INVALID" }),
                        ),
                    );
                }
            }
        }
        if let Err(message) = crate::orchestrator::validate_case_data_set(&draft_val) {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({ "error": message, "code": "CASE_DATASET_INVALID" })),
            );
        }
    } else if asset.kind == "suite" {
        let cases = draft_val.get("cases").and_then(Value::as_array);
        let Some(cases) = cases.filter(|cases| !cases.is_empty()) else {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(
                    json!({ "error": "A suite must contain at least one published case revision" }),
                ),
            );
        };
        for case_ref in cases {
            let case_id = case_ref
                .get("case_id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let revision_id = case_ref
                .get("revision_id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let valid: Option<(String, String)> = sqlx::query_as(
                "SELECT assets.kind, assets.workspace_id FROM asset_revisions
                 JOIN assets ON assets.id = asset_revisions.asset_id
                 WHERE asset_revisions.id = ? AND assets.id = ?",
            )
            .bind(revision_id)
            .bind(case_id)
            .fetch_optional(&state.db)
            .await
            .unwrap_or(None);
            if !matches!(valid, Some((ref kind, ref workspace)) if kind == "case" && workspace == &asset.workspace_id)
            {
                return (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(
                        json!({ "error": "Each suite case must reference a published case revision in this workspace" }),
                    ),
                );
            }
        }
    }

    // 3. Determine next revision version number
    let next_version: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(version), 0) + 1 FROM asset_revisions WHERE asset_id = ?",
    )
    .bind(&asset_id)
    .fetch_one(&state.db)
    .await
    .unwrap_or(1);

    // 4. Compute canonical checksum
    let checksum = compute_sha256(asset.draft_json.as_bytes());
    let revision_id = Uuid::new_v4().to_string();
    let change_note = payload.change_note.unwrap_or_default();
    let now = Utc::now().to_rfc3339();

    // 5. Insert revision
    let mut tx = match state.db.begin().await {
        Ok(tx) => tx,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({ "error": format!("Could not begin revision transaction: {}", error) }),
                ),
            )
        }
    };
    let insert_res = sqlx::query(
        "INSERT INTO asset_revisions (id, asset_id, version, definition_json, checksum, change_note, author_id, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&revision_id)
    .bind(&asset_id)
    .bind(next_version)
    .bind(&asset.draft_json)
    .bind(&checksum)
    .bind(&change_note)
    .bind(&user.id)
    .bind(&now)
    .execute(&mut *tx)
    .await;

    if let Err(error) = &insert_res {
        let _ = tx.rollback().await;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        );
    }

    if asset.kind == "suite" {
        if let Some(cases) = draft_val.get("cases").and_then(Value::as_array) {
            for (index, case_ref) in cases.iter().enumerate() {
                let Some(dependency_id) = case_ref.get("revision_id").and_then(Value::as_str)
                else {
                    continue;
                };
                let _ = sqlx::query(
                    "INSERT OR IGNORE INTO asset_dependencies (revision_id, dependency_revision_id, alias) VALUES (?, ?, ?)",
                )
                .bind(&revision_id)
                .bind(dependency_id)
                .bind(format!("case_{}", index + 1))
                .execute(&mut *tx)
                .await;
            }
        }
    }

    if let Err(error) = tx.commit().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Revision transaction failed: {}", error) })),
        );
    }

    (
        StatusCode::OK,
        Json(json!(PublishDraftResponse {
            revision_id,
            version: next_version,
            checksum,
            published_at: now,
        })),
    )
}

pub async fn list_revisions(
    State(state): State<AppState>,
    Path(asset_id): Path<String>,
    Extension(user): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let revs = sqlx::query_as::<_, AssetRevision>(
        "SELECT id, asset_id, version, definition_json, checksum, change_note, author_id, created_at 
         FROM asset_revisions WHERE asset_id = ? AND EXISTS (
             SELECT 1 FROM assets WHERE assets.id = asset_revisions.asset_id AND assets.workspace_id = ?
         ) ORDER BY version DESC"
    )
    .bind(&asset_id)
    .bind(&user.workspace_id)
    .fetch_all(&state.db)
    .await;

    match revs {
        Ok(list) => (StatusCode::OK, Json(json!(list))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

pub async fn get_revision(
    State(state): State<AppState>,
    Path((_asset_id, revision_id)): Path<(String, String)>,
    Extension(user): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let rev = sqlx::query_as::<_, AssetRevision>(
        "SELECT id, asset_id, version, definition_json, checksum, change_note, author_id, created_at 
         FROM asset_revisions WHERE id = ? AND asset_id = ? AND EXISTS (
             SELECT 1 FROM assets WHERE assets.id = asset_revisions.asset_id AND assets.workspace_id = ?
         )"
    )
    .bind(&revision_id)
    .bind(_asset_id)
    .bind(&user.workspace_id)
    .fetch_optional(&state.db)
    .await;

    match rev {
        Ok(Some(r)) => {
            let def: Value = serde_json::from_str(&r.definition_json).unwrap_or(json!({}));
            (
                StatusCode::OK,
                Json(json!({
                    "id": r.id,
                    "asset_id": r.asset_id,
                    "version": r.version,
                    "checksum": r.checksum,
                    "change_note": r.change_note,
                    "definition": def,
                    "created_at": r.created_at
                })),
            )
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Revision not found" })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

/// Cycle detection via Kahn's algorithm
fn detect_cycles(nodes: &[Value], edges: &[Value]) -> Result<(), String> {
    use std::collections::{HashMap, HashSet, VecDeque};

    let mut in_degrees: HashMap<String, usize> = HashMap::new();
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();

    for n in nodes {
        if let Some(id) = n.get("id").and_then(|i| i.as_str()) {
            in_degrees.insert(id.to_string(), 0);
            adj.insert(id.to_string(), Vec::new());
        }
    }

    for e in edges {
        let src = e.get("source").and_then(|s| s.as_str());
        let tgt = e.get("target").and_then(|t| t.as_str());
        if let (Some(s), Some(t)) = (src, tgt) {
            adj.entry(s.to_string()).or_default().push(t.to_string());
            *in_degrees.entry(t.to_string()).or_insert(0) += 1;
        }
    }

    let mut queue = VecDeque::new();
    for (id, deg) in &in_degrees {
        if *deg == 0 {
            queue.push_back(id.clone());
        }
    }

    let mut visited = 0;
    while let Some(node) = queue.pop_front() {
        visited += 1;
        if let Some(neighbors) = adj.get(&node) {
            for neighbor in neighbors {
                if let Some(deg) = in_degrees.get_mut(neighbor) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(neighbor.clone());
                    }
                }
            }
        }
    }

    if visited < in_degrees.len() {
        Err(format!(
            "Cycle detected among nodes. Visited {}/{} nodes.",
            visited,
            in_degrees.len()
        ))
    } else {
        Ok(())
    }
}
