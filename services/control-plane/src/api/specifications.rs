use axum::{extract::State, http::StatusCode, response::IntoResponse, Extension, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{api::auth::AuthenticatedUser, AppState};

#[derive(Deserialize)]
pub struct ValidateOpenApiRequest {
    pub spec_content: String,
}

#[derive(Deserialize)]
pub struct ImportOpenApiRequest {
    pub spec_content: String,
    pub selected_operation_ids: Vec<String>,
}

pub async fn validate_openapi(Json(payload): Json<ValidateOpenApiRequest>) -> impl IntoResponse {
    let parsed: Value = match serde_json::from_str(&payload.spec_content) {
        Ok(v) => v,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "Invalid OpenAPI JSON format" })),
            )
        }
    };

    let title = parsed
        .get("info")
        .and_then(|i| i.get("title"))
        .and_then(|t| t.as_str())
        .unwrap_or("Imported API Specification");

    let version = parsed
        .get("info")
        .and_then(|i| i.get("version"))
        .and_then(|v| v.as_str())
        .unwrap_or("1.0.0");

    let mut operations = Vec::new();

    if let Some(paths) = parsed.get("paths").and_then(|p| p.as_object()) {
        for (path_str, path_item) in paths {
            if let Some(methods) = path_item.as_object() {
                for (method_str, op_obj) in methods {
                    let method_upper = method_str.to_uppercase();
                    if matches!(
                        method_upper.as_str(),
                        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD"
                    ) {
                        let op_id = op_obj
                            .get("operationId")
                            .and_then(|id| id.as_str())
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| {
                                format!(
                                    "{}_{}",
                                    method_upper.to_lowercase(),
                                    path_str.replace('/', "_")
                                )
                            });

                        let summary = op_obj
                            .get("summary")
                            .and_then(|s| s.as_str())
                            .unwrap_or(path_str);

                        operations.push(json!({
                            "operation_id": op_id,
                            "method": method_upper,
                            "path": path_str,
                            "summary": summary
                        }));
                    }
                }
            }
        }
    }

    (
        StatusCode::OK,
        Json(json!({
            "is_valid": true,
            "title": title,
            "version": version,
            "total_operations": operations.len(),
            "operations": operations
        })),
    )
}

pub async fn import_openapi(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(payload): Json<ImportOpenApiRequest>,
) -> impl IntoResponse {
    let parsed: Value = match serde_json::from_str(&payload.spec_content) {
        Ok(v) => v,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "Invalid OpenAPI JSON format" })),
            )
        }
    };

    let mut created_templates = Vec::new();

    if let Some(paths) = parsed.get("paths").and_then(|p| p.as_object()) {
        for (path_str, path_item) in paths {
            if let Some(methods) = path_item.as_object() {
                for (method_str, op_obj) in methods {
                    let method_upper = method_str.to_uppercase();
                    let op_id = op_obj
                        .get("operationId")
                        .and_then(|id| id.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| {
                            format!(
                                "{}_{}",
                                method_upper.to_lowercase(),
                                path_str.replace('/', "_")
                            )
                        });

                    if payload.selected_operation_ids.contains(&op_id) {
                        let template_id = Uuid::new_v4().to_string();
                        let summary = op_obj
                            .get("summary")
                            .and_then(|s| s.as_str())
                            .unwrap_or(path_str);

                        let template_node = json!({
                            "type": "api.request",
                            "type_version": 1,
                            "name": summary,
                            "timeout_seconds": 30,
                            "config": {
                                "method": method_upper,
                                "path": path_str,
                                "headers": { "Accept": "application/json" }
                            }
                        });

                        // Insert as template asset
                        let now = chrono::Utc::now().to_rfc3339();
                        let _ = sqlx::query(
                            "INSERT INTO assets (id, workspace_id, kind, name, description, draft_json, draft_version, created_at, updated_at)
                             VALUES (?, ?, 'template', ?, ?, ?, 1, ?, ?)"
                        )
                        .bind(&template_id)
                        .bind(&user.workspace_id)
                        .bind(format!("OpenAPI: {}", summary))
                        .bind(format!("Auto-generated from operation {}", op_id))
                        .bind(template_node.to_string())
                        .bind(&now)
                        .bind(&now)
                        .execute(&state.db)
                        .await;

                        created_templates.push(json!({
                            "template_id": template_id,
                            "operation_id": op_id,
                            "name": summary
                        }));
                    }
                }
            }
        }
    }

    (
        StatusCode::CREATED,
        Json(json!({
            "imported_count": created_templates.len(),
            "templates": created_templates
        })),
    )
}
