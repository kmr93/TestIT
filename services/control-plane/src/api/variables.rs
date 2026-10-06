use crate::{
    api::auth::AuthenticatedUser,
    models::{EnvironmentRecord, VariablePreviewRequest, VariablePreviewResponse},
    variables::{resolve_variable_definitions, ResolutionContext, VariableResolver},
    AppState,
};
use axum::{extract::State, http::StatusCode, response::IntoResponse, Extension, Json};
use serde_json::{json, Value};

pub async fn preview_variables(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(payload): Json<VariablePreviewRequest>,
) -> impl IntoResponse {
    let mut context = ResolutionContext::default();
    let mut errors = Vec::new();
    let mut masked_secrets = Vec::new();

    // 1. If environment_id provided, load environment variables
    if let Some(env_id) = payload.environment_id {
        let env_res = sqlx::query_as::<_, EnvironmentRecord>(
            "SELECT id, workspace_id, name, description, variables_json, created_at FROM environments WHERE id = ? AND workspace_id = ?"
        )
        .bind(&env_id)
        .bind(&user.workspace_id)
        .fetch_optional(&state.db)
        .await;

        match env_res {
            Ok(Some(env)) => match serde_json::from_str::<Value>(&env.variables_json) {
                Ok(definitions) => {
                    match resolve_variable_definitions(Some(&definitions), &context, "env") {
                        Ok(values) => context.env_vars = values,
                        Err(error) => errors.push(format!("Environment variables: {error}")),
                    }
                }
                Err(_) => errors.push("Environment variables are invalid JSON".to_string()),
            },
            Ok(None) => errors.push("The selected environment is unavailable".to_string()),
            Err(_) => errors.push("The selected environment could not be loaded".to_string()),
        }
    }

    // 2. Load registered secrets as masked placeholders
    let secrets_res =
        sqlx::query_scalar::<_, String>("SELECT name FROM secrets WHERE workspace_id = ?")
            .bind(&user.workspace_id)
            .fetch_all(&state.db)
            .await;

    if let Ok(sec_names) = secrets_res {
        for name in sec_names {
            masked_secrets.push(name.clone());
            context
                .secrets
                .insert(name.clone(), format!("[SECRET:{}]", name));
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

    if let Some(definitions) = payload.suite_variables {
        match resolve_variable_definitions(Some(&definitions), &context, "suite") {
            Ok(values) => context.suite_vars = values,
            Err(error) => errors.push(format!("Suite variables: {error}")),
        }
    }
    if let Some(definitions) = payload.case_variables {
        match resolve_variable_definitions(Some(&definitions), &context, "case") {
            Ok(values) => context.case_vars.extend(values),
            Err(error) => errors.push(format!("Case variables: {error}")),
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
