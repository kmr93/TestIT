use axum::{extract::State, http::StatusCode, response::IntoResponse, Extension, Json};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use uuid::Uuid;

use crate::{api::auth::AuthenticatedUser, crypto::compute_sha256, AppState};

const MAX_SPEC_BYTES: usize = 5 * 1024 * 1024;
const MAX_OPERATIONS: usize = 1000;
const MAX_IMPORT_OPERATIONS: usize = 100;

#[derive(Deserialize)]
pub struct ValidateOpenApiRequest {
    pub spec_content: String,
}

#[derive(Deserialize)]
pub struct ImportOpenApiRequest {
    pub spec_content: String,
    pub selected_operation_ids: Vec<String>,
}

#[derive(Debug)]
struct OpenApiOperation {
    id: String,
    method: String,
    path: String,
    summary: String,
    server_url: Option<String>,
    parameters: Vec<Value>,
    request_schema: Option<Value>,
    response_schema: Option<Value>,
    expected_status: i64,
}

pub async fn validate_openapi(Json(payload): Json<ValidateOpenApiRequest>) -> impl IntoResponse {
    match parse_openapi(&payload.spec_content) {
        Ok((document, operations)) => {
            let checksum = compute_sha256(payload.spec_content.as_bytes());
            let public_operations = operations
                .iter()
                .map(|operation| {
                    json!({
                        "operation_id": operation.id,
                        "method": operation.method,
                        "path": operation.path,
                        "summary": operation.summary,
                        "parameters": operation.parameters.iter().map(|parameter| json!({
                            "name": parameter.get("name").and_then(Value::as_str).unwrap_or_default(),
                            "in": parameter.get("in").and_then(Value::as_str).unwrap_or_default(),
                            "required": parameter.get("required").and_then(Value::as_bool).unwrap_or(false)
                        })).collect::<Vec<_>>(),
                        "has_request_schema": operation.request_schema.is_some(),
                        "has_response_schema": operation.response_schema.is_some(),
                        "expected_status": operation.expected_status
                    })
                })
                .collect::<Vec<_>>();
            (
                StatusCode::OK,
                Json(json!({
                    "is_valid": true,
                    "title": document.pointer("/info/title").and_then(Value::as_str).unwrap_or("Imported API Specification"),
                    "version": document.pointer("/info/version").and_then(Value::as_str).unwrap_or("1.0.0"),
                    "openapi_version": document.get("openapi").and_then(Value::as_str).unwrap_or_default(),
                    "source_checksum": checksum,
                    "total_operations": public_operations.len(),
                    "operations": public_operations
                })),
            )
        }
        Err(message) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "is_valid": false, "error": message })),
        ),
    }
}

pub async fn import_openapi(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(payload): Json<ImportOpenApiRequest>,
) -> impl IntoResponse {
    if payload.selected_operation_ids.is_empty()
        || payload.selected_operation_ids.len() > MAX_IMPORT_OPERATIONS
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Select between 1 and 100 operations to import" })),
        );
    }
    let (document, operations) = match parse_openapi(&payload.spec_content) {
        Ok(value) => value,
        Err(message) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({ "error": message })),
            )
        }
    };
    let requested = payload
        .selected_operation_ids
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    if requested.len() != payload.selected_operation_ids.len()
        || requested
            .iter()
            .any(|id| !operations.iter().any(|op| &op.id == id))
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                json!({ "error": "The operation selection contains duplicate or unknown operation IDs" }),
            ),
        );
    }

    let source_checksum = compute_sha256(payload.spec_content.as_bytes());
    let source_version = document
        .get("openapi")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let source_title = document
        .pointer("/info/title")
        .and_then(Value::as_str)
        .unwrap_or("Imported API Specification");
    let schema_components = document
        .pointer("/components/schemas")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let now = chrono::Utc::now().to_rfc3339();
    let mut transaction = match state.db.begin().await {
        Ok(value) => value,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Could not begin the OpenAPI import transaction" })),
            )
        }
    };
    let mut created_cases = Vec::new();
    for operation in operations
        .iter()
        .filter(|operation| requested.contains(&operation.id))
    {
        let asset_id = Uuid::new_v4().to_string();
        let case_id = Uuid::new_v4().to_string();
        let node_id = Uuid::new_v4().to_string();
        let name = format!("{} {}", operation.method, operation.path);
        let mut headers = Map::new();
        headers.insert("Accept".to_string(), json!("application/json"));
        let mut query = Map::new();
        let mut path_parameters = Map::new();
        let mut required_parameters = Vec::new();
        for parameter in &operation.parameters {
            let parameter_name = parameter
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if parameter.get("required").and_then(Value::as_bool) == Some(true) {
                required_parameters.push(json!({
                    "name": parameter_name,
                    "in": parameter.get("in").and_then(Value::as_str).unwrap_or_default()
                }));
            }
            match parameter
                .get("in")
                .and_then(Value::as_str)
                .unwrap_or_default()
            {
                "path" => {
                    path_parameters.insert(parameter_name.to_string(), json!(""));
                }
                "query" => {
                    query.insert(parameter_name.to_string(), json!(""));
                }
                "header"
                    if !matches!(
                        parameter_name.to_ascii_lowercase().as_str(),
                        "authorization" | "cookie" | "proxy-authorization" | "set-cookie"
                    ) =>
                {
                    headers.insert(parameter_name.to_string(), json!(""));
                }
                _ => {}
            }
        }
        let mut config = json!({
            "method": operation.method,
            "path": operation.path,
            "headers": headers,
            "query": query,
            "path_parameters": path_parameters,
            "required_parameters": required_parameters,
            "expected_status": operation.expected_status,
            "assertions": [],
            "extract": {}
        });
        if let Some(server_url) = operation.server_url.as_deref() {
            if let Some(config) = config.as_object_mut() {
                config.insert("base_url".to_string(), json!(server_url));
            }
        }
        if let Some(body_schema) = &operation.request_schema {
            if let Some(config) = config.as_object_mut() {
                config.insert("body".to_string(), json!({}));
                config.insert("request_schema".to_string(), body_schema.clone());
                config.insert(
                    "request_schema_components".to_string(),
                    schema_components.clone(),
                );
                config.insert("openapi_version".to_string(), json!(source_version));
            }
        }
        if let Some(response_schema) = &operation.response_schema {
            if let Some(config) = config.as_object_mut() {
                config.insert("response_schema".to_string(), response_schema.clone());
                config.insert(
                    "response_schema_components".to_string(),
                    schema_components.clone(),
                );
                config.insert("openapi_version".to_string(), json!(source_version));
            }
        }
        let definition = json!({
            "id": case_id,
            "name": name,
            "description": operation.summary,
            "variables": {},
            "nodes": [{
                "id": node_id,
                "type": "api.request",
                "type_version": 1,
                "name": operation.summary,
                "phase": "main",
                "timeout_seconds": 30,
                "config": config
            }],
            "edges": [],
            "openapi_import": {
                "source_title": source_title,
                "source_version": source_version,
                "source_checksum": source_checksum,
                "operation_id": operation.id,
                "importer_version": 1
            }
        });
        let insert = sqlx::query(
            "INSERT INTO assets (id, workspace_id, kind, name, description, draft_json, draft_version, created_at, updated_at)
             VALUES (?, ?, 'case', ?, ?, ?, 1, ?, ?)",
        )
        .bind(&asset_id)
        .bind(&user.workspace_id)
        .bind(&name)
        .bind(&operation.summary)
        .bind(definition.to_string())
        .bind(&now)
        .bind(&now)
        .execute(&mut *transaction)
        .await;
        if insert.is_err() {
            let _ = transaction.rollback().await;
            return (
                StatusCode::CONFLICT,
                Json(
                    json!({ "error": "One or more imported case drafts could not be created; no operations were imported" }),
                ),
            );
        }
        created_cases.push(json!({
            "asset_id": asset_id,
            "operation_id": operation.id,
            "name": name
        }));
    }
    let audit = sqlx::query(
        "INSERT INTO audit_events (id, actor_id, action, target_type, target_id, changes_json, created_at)
         VALUES (?, ?, 'openapi.import', 'openapi_import', ?, ?, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(&user.id)
    .bind(&source_checksum)
    .bind(json!({
        "source_title": source_title,
        "source_version": source_version,
        "operation_ids": created_cases.iter().filter_map(|case| case.get("operation_id").and_then(Value::as_str)).collect::<Vec<_>>(),
        "asset_ids": created_cases.iter().filter_map(|case| case.get("asset_id").and_then(Value::as_str)).collect::<Vec<_>>()
    }).to_string())
    .bind(&now)
    .execute(&mut *transaction)
    .await;
    if audit.is_err() {
        let _ = transaction.rollback().await;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                json!({ "error": "Could not audit the OpenAPI import; no operations were imported" }),
            ),
        );
    }
    if transaction.commit().await.is_err() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Could not save the imported case drafts" })),
        );
    }
    (
        StatusCode::CREATED,
        Json(json!({
            "imported_count": created_cases.len(),
            "source_checksum": source_checksum,
            "cases": created_cases
        })),
    )
}

fn parse_openapi(content: &str) -> Result<(Value, Vec<OpenApiOperation>), String> {
    if content.len() > MAX_SPEC_BYTES {
        return Err("OpenAPI JSON exceeds the 5 MiB import limit".to_string());
    }
    let document: Value = serde_json::from_str(content)
        .map_err(|_| "OpenAPI specification must be valid JSON".to_string())?;
    let version = document
        .get("openapi")
        .and_then(Value::as_str)
        .ok_or_else(|| "An OpenAPI 3.0 or 3.1 version is required".to_string())?;
    if !(version.starts_with("3.0.") || version.starts_with("3.1.")) {
        return Err("Only OpenAPI 3.0 and 3.1 specifications are supported".to_string());
    }
    if document
        .pointer("/info/title")
        .and_then(Value::as_str)
        .is_none_or(|title| title.trim().is_empty() || title.len() > 256)
        || document
            .pointer("/info/version")
            .and_then(Value::as_str)
            .is_none_or(|version| version.trim().is_empty() || version.len() > 64)
    {
        return Err("OpenAPI info requires a bounded title and version".to_string());
    }
    if has_remote_reference(&document) {
        return Err("Remote OpenAPI references are not fetched; replace them with local references before importing".to_string());
    }
    validate_local_references(&document, &document)?;
    let paths = document
        .get("paths")
        .and_then(Value::as_object)
        .ok_or_else(|| "OpenAPI paths must be an object".to_string())?;
    if paths.is_empty() || paths.len() > MAX_OPERATIONS {
        return Err("OpenAPI must contain between 1 and 1000 paths".to_string());
    }
    let mut operations = Vec::new();
    let mut seen_ids = HashSet::new();
    for (path, path_item) in paths {
        if !path.starts_with('/') {
            return Err("Every OpenAPI path must start with '/'".to_string());
        }
        let path_item = resolve_local_ref(&document, path_item)?;
        let methods = path_item
            .as_object()
            .ok_or_else(|| format!("OpenAPI path '{path}' must contain an object"))?;
        for (method, operation) in methods {
            let method = method.to_ascii_uppercase();
            if !matches!(
                method.as_str(),
                "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS"
            ) {
                continue;
            }
            let operation = resolve_local_ref(&document, operation)?;
            if !operation.is_object() {
                return Err(format!(
                    "OpenAPI operation '{method} {path}' must be an object"
                ));
            }
            let operation_id = operation
                .get("operationId")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| {
                    format!("{}_{}", method.to_ascii_lowercase(), path.replace('/', "_"))
                });
            if operation_id.trim().is_empty()
                || operation_id.len() > 128
                || !seen_ids.insert(operation_id.clone())
            {
                return Err("Every OpenAPI operation must have a unique operation ID of at most 128 characters".to_string());
            }
            let summary = operation
                .get("summary")
                .and_then(Value::as_str)
                .or_else(|| operation.get("description").and_then(Value::as_str))
                .unwrap_or(&operation_id)
                .trim();
            let summary = if summary.is_empty() {
                &operation_id
            } else {
                summary
            };
            let summary = summary.chars().take(128).collect::<String>();
            let mut parameters = Vec::new();
            for source in [path_item.get("parameters"), operation.get("parameters")]
                .into_iter()
                .flatten()
            {
                let source = source
                    .as_array()
                    .ok_or_else(|| "OpenAPI parameters must be arrays".to_string())?;
                for parameter in source {
                    let parameter = resolve_local_ref(&document, parameter)?;
                    let name = parameter
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let location = parameter
                        .get("in")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if name.is_empty()
                        || name.len() > 128
                        || !matches!(location, "path" | "query" | "header" | "cookie")
                    {
                        return Err(format!(
                            "OpenAPI operation '{operation_id}' contains an invalid parameter"
                        ));
                    }
                    if location != "cookie" {
                        parameters.push(parameter);
                    }
                }
            }
            let server_url = first_server_url(operation.get("servers"))
                .or_else(|| first_server_url(path_item.get("servers")))
                .or_else(|| first_server_url(document.get("servers")));
            let request_schema = operation
                .get("requestBody")
                .map(|request| resolve_local_ref(&document, request))
                .transpose()?
                .and_then(|request| {
                    request
                        .pointer("/content/application~1json/schema")
                        .cloned()
                });
            let response_schema = response_schema(&document, &operation)?;
            let expected_status = expected_success_status(&operation);
            operations.push(OpenApiOperation {
                id: operation_id,
                method,
                path: path.clone(),
                summary,
                server_url,
                parameters,
                request_schema,
                response_schema,
                expected_status,
            });
            if operations.len() > MAX_OPERATIONS {
                return Err("OpenAPI contains more than 1000 operations".to_string());
            }
        }
    }
    if operations.is_empty() {
        return Err("OpenAPI does not contain any supported HTTP operations".to_string());
    }
    Ok((document, operations))
}

fn resolve_local_ref(document: &Value, value: &Value) -> Result<Value, String> {
    let Some(reference) = value.get("$ref").and_then(Value::as_str) else {
        return Ok(value.clone());
    };
    if !reference.starts_with("#/") {
        return Err("Only local OpenAPI JSON Pointer references are supported".to_string());
    }
    document
        .pointer(&reference[1..])
        .cloned()
        .ok_or_else(|| format!("OpenAPI reference '{reference}' does not exist"))
}

fn validate_local_references(root: &Value, value: &Value) -> Result<(), String> {
    match value {
        Value::Object(object) => {
            if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
                if !reference.starts_with("#/") || root.pointer(&reference[1..]).is_none() {
                    return Err(format!("OpenAPI reference '{reference}' must point to an existing local JSON Pointer"));
                }
            }
            for child in object.values() {
                validate_local_references(root, child)?;
            }
        }
        Value::Array(values) => {
            for child in values {
                validate_local_references(root, child)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn has_remote_reference(value: &Value) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(key, value)| {
            (key == "$ref"
                && value.as_str().is_some_and(|reference| {
                    reference.starts_with("http:")
                        || reference.starts_with("https:")
                        || reference.starts_with("//")
                        || reference.starts_with("file:")
                }))
                || has_remote_reference(value)
        }),
        Value::Array(values) => values.iter().any(has_remote_reference),
        _ => false,
    }
}

fn first_server_url(value: Option<&Value>) -> Option<String> {
    let url = value?
        .as_array()?
        .first()?
        .get("url")?
        .as_str()
        .filter(|url| !url.trim().is_empty() && !url.contains('{'))?;
    let authority = url
        .split_once("://")
        .map(|(_, rest)| rest.split('/').next().unwrap_or_default())?;
    let query = url
        .split_once('?')
        .map(|(_, query)| query.split('#').next().unwrap_or_default())
        .unwrap_or_default();
    let has_credential_query = query.split('&').any(|pair| {
        let key = pair
            .split('=')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        [
            "token",
            "key",
            "password",
            "secret",
            "authorization",
            "cookie",
            "credential",
        ]
        .iter()
        .any(|marker| key.contains(marker))
    });
    if authority.contains('@') || has_credential_query {
        return None;
    }
    Some(url.to_string())
}

fn response_schema(document: &Value, operation: &Value) -> Result<Option<Value>, String> {
    let Some(responses) = operation.get("responses").and_then(Value::as_object) else {
        return Ok(None);
    };
    let preferred = ["200", "201", "202", "203", "204", "default"];
    for code in preferred {
        if let Some(response) = responses.get(code) {
            if let Some(schema) =
                resolve_local_ref(document, response)?.pointer("/content/application~1json/schema")
            {
                return Ok(Some(schema.clone()));
            }
        }
    }
    for (_, response) in responses.iter().filter(|(code, _)| code.starts_with('2')) {
        if let Some(schema) =
            resolve_local_ref(document, response)?.pointer("/content/application~1json/schema")
        {
            return Ok(Some(schema.clone()));
        }
    }
    Ok(None)
}

fn expected_success_status(operation: &Value) -> i64 {
    let Some(responses) = operation.get("responses").and_then(Value::as_object) else {
        return 200;
    };
    for preferred in ["200", "201", "202", "204"] {
        if responses.contains_key(preferred) {
            return preferred.parse().unwrap_or(200);
        }
    }
    responses
        .keys()
        .filter_map(|code| code.parse::<i64>().ok())
        .find(|code| (200..300).contains(code))
        .unwrap_or(200)
}

#[cfg(test)]
mod tests {
    use super::parse_openapi;

    #[test]
    fn parses_operations_and_retains_parameters_and_schema() {
        let spec = r#"{
          "openapi":"3.1.0",
          "info":{"title":"Example API","version":"1.2"},
          "servers":[{"url":"https://api.example.test"}],
          "paths":{"/users/{userId}":{
            "parameters":[{"name":"userId","in":"path","required":true}],
            "get":{"operationId":"getUser","summary":"Get user","responses":{"200":{"content":{"application/json":{"schema":{"type":"object","properties":{"id":{"type":"string"}}}}}}}}
          }}
        }"#;

        let (document, operations) = parse_openapi(spec).unwrap();
        assert_eq!(document["info"]["title"], "Example API");
        assert_eq!(operations.len(), 1);
        assert_eq!(operations[0].expected_status, 200);
        assert_eq!(operations[0].parameters[0]["name"], "userId");
        assert!(operations[0].response_schema.is_some());
        assert_eq!(
            operations[0].server_url.as_deref(),
            Some("https://api.example.test")
        );
    }

    #[test]
    fn rejects_remote_references_and_duplicate_operation_ids() {
        let remote = r##"{"openapi":"3.0.0","info":{"title":"A","version":"1"},"paths":{"/a":{"get":{"operationId":"a","responses":{}}}},"components":{"schemas":{"A":{"$ref":"https://example.test/schema.json"}}}}"##;
        assert!(parse_openapi(remote)
            .unwrap_err()
            .contains("Remote OpenAPI references"));

        let duplicate = r#"{"openapi":"3.0.0","info":{"title":"A","version":"1"},"paths":{"/a":{"get":{"operationId":"same","responses":{}}},"/b":{"get":{"operationId":"same","responses":{}}}}}"#;
        assert!(parse_openapi(duplicate)
            .unwrap_err()
            .contains("unique operation ID"));
    }
}
