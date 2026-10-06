use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::{
    models::{EnvironmentRecord, VariablePreviewRequest, VariablePreviewResponse},
    variables::{ResolutionContext, VariableResolver},
    AppState,
};

pub async fn preview_variables(
    State(state): State<AppState>,
    Json(payload): Json<VariablePreviewRequest>,
) -> impl IntoResponse {
    let mut context = ResolutionContext::default();
    let mut errors = Vec::new();
    let mut masked_secrets = Vec::new();

    // 1. If environment_id provided, load environment variables
    if let Some(env_id) = payload.environment_id {
        let env_res = sqlx::query_as::<_, EnvironmentRecord>(
            "SELECT id, workspace_id, name, description, variables_json, created_at FROM environments WHERE id = ?"
        )
        .bind(&env_id)
        .fetch_optional(&state.db)
        .await;

        if let Ok(Some(env)) = env_res {
            if let Ok(vars_map) = serde_json::from_str::<HashMap<String, Value>>(&env.variables_json) {
                context.env_vars = vars_map;
            }
        }
    }

    // 2. Load registered secrets as masked placeholders
    let secrets_res = sqlx::query_scalar::<_, String>(
        "SELECT name FROM secrets"
    )
    .fetch_all(&state.db)
    .await;

    if let Ok(sec_names) = secrets_res {
        for name in sec_names {
            masked_secrets.push(name.clone());
            context.secrets.insert(name.clone(), format!("[SECRET:{}]", name));
        }
    }

    // 3. Inject sample case inputs
    if let Some(inputs) = payload.sample_inputs {
        if let Some(map) = inputs.as_object() {
            for (k, v) in map {
                context.case_vars.insert(k.clone(), v.clone());
            }
        }
    }

    // 4. Inject sample iteration data row
    if let Some(iter_data) = payload.sample_iteration {
        if let Some(map) = iter_data.as_object() {
            for (k, v) in map {
                context.iteration_vars.insert(k.clone(), v.clone());
            }
        }
    }

    // 5. Run deterministic resolver
    let mut resolver = VariableResolver::new(&context);
    let resolved_config = match resolver.resolve_json_value(&payload.node_config) {
        Ok(v) => v,
        Err(e) => {
            errors.push(e);
            payload.node_config
        }
    };

    (
        StatusCode::OK,
        Json(json!(VariablePreviewResponse {
            network_accessed: false,
            resolved_config,
            masked_secrets,
            errors,
        })),
    )
}
