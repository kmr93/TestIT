use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{
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

pub async fn list_assets(State(state): State<AppState>) -> impl IntoResponse {
    let assets = sqlx::query_as::<_, Asset>(
        "SELECT id, workspace_id, kind, name, description, draft_json, draft_version, archived_at, created_at, updated_at 
         FROM assets WHERE archived_at IS NULL ORDER BY updated_at DESC"
    )
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
    Json(payload): Json<CreateAssetRequest>,
) -> impl IntoResponse {
    let id = Uuid::new_v4().to_string();
    let workspace_id = "00000000-0000-0000-0000-000000000001"; // default v1 workspace
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
    .bind(workspace_id)
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
) -> impl IntoResponse {
    let asset = sqlx::query_as::<_, Asset>(
        "SELECT id, workspace_id, kind, name, description, draft_json, draft_version, archived_at, created_at, updated_at 
         FROM assets WHERE id = ?"
    )
    .bind(&asset_id)
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
    Json(payload): Json<UpdateDraftRequest>,
) -> impl IntoResponse {
    let now = Utc::now().to_rfc3339();
    let draft_str = payload.draft_json.to_string();

    // Optimistic concurrency check on draft_version
    let res = sqlx::query(
        "UPDATE assets 
         SET draft_json = ?, draft_version = draft_version + 1, updated_at = ?,
             name = COALESCE(?, name), description = COALESCE(?, description)
         WHERE id = ? AND draft_version = ?"
    )
    .bind(&draft_str)
    .bind(&now)
    .bind(&payload.name)
    .bind(&payload.description)
    .bind(&asset_id)
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
    Json(payload): Json<PublishDraftRequest>,
) -> impl IntoResponse {
    // 1. Fetch current asset draft
    let asset = match sqlx::query_as::<_, Asset>(
        "SELECT id, workspace_id, kind, name, description, draft_json, draft_version, archived_at, created_at, updated_at 
         FROM assets WHERE id = ?"
    )
    .bind(&asset_id)
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

    if asset.kind == "case" {
        if let Some(nodes) = draft_val.get("nodes").and_then(|n| n.as_array()) {
            if let Some(edges) = draft_val.get("edges").and_then(|e| e.as_array()) {
                if let Err(cycle_err) = detect_cycles(nodes, edges) {
                    return (
                        StatusCode::UNPROCESSABLE_ENTITY,
                        Json(json!({
                            "error": format!("Workflow graph contains cycles: {}", cycle_err),
                            "code": "GRAPH_CYCLE_DETECTED"
                        })),
                    );
                }
            }
        }
    }

    // 3. Determine next revision version number
    let next_version: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(version), 0) + 1 FROM asset_revisions WHERE asset_id = ?"
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
    let insert_res = sqlx::query(
        "INSERT INTO asset_revisions (id, asset_id, version, definition_json, checksum, change_note, author_id, created_at)
         VALUES (?, ?, ?, ?, ?, ?, NULL, ?)"
    )
    .bind(&revision_id)
    .bind(&asset_id)
    .bind(next_version)
    .bind(&asset.draft_json)
    .bind(&checksum)
    .bind(&change_note)
    .bind(&now)
    .execute(&state.db)
    .await;

    match insert_res {
        Ok(_) => (
            StatusCode::OK,
            Json(json!(PublishDraftResponse {
                revision_id,
                version: next_version,
                checksum,
                published_at: now,
            })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

pub async fn list_revisions(
    State(state): State<AppState>,
    Path(asset_id): Path<String>,
) -> impl IntoResponse {
    let revs = sqlx::query_as::<_, AssetRevision>(
        "SELECT id, asset_id, version, definition_json, checksum, change_note, author_id, created_at 
         FROM asset_revisions WHERE asset_id = ? ORDER BY version DESC"
    )
    .bind(&asset_id)
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
) -> impl IntoResponse {
    let rev = sqlx::query_as::<_, AssetRevision>(
        "SELECT id, asset_id, version, definition_json, checksum, change_note, author_id, created_at 
         FROM asset_revisions WHERE id = ?"
    )
    .bind(&revision_id)
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
