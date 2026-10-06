pub mod admin;
pub mod assets;
pub mod auth;
pub mod connections;
pub mod events;
pub mod health;
pub mod portability;
pub mod runs;
pub mod specifications;
pub mod variables;

use crate::AppState;
use axum::{
    middleware,
    routing::{get, patch, post},
    Router,
};

pub fn create_router(state: AppState) -> Router {
    let public = Router::new()
        // Health Probes
        .route("/health/live", get(health::liveness_check))
        .route("/health/ready", get(health::readiness_check))
        .route("/metrics", get(health::metrics))
        .route("/api/v1/auth/login", post(auth::login));

    let protected = Router::new()
        .route("/api/v1/me", get(auth::get_current_user))
        .route("/api/v1/auth/logout", post(auth::logout))
        .route(
            "/api/v1/admin/users",
            get(admin::list_users).post(admin::create_user),
        )
        .route("/api/v1/admin/users/:user_id", patch(admin::update_user))
        // Assets & Revisions
        .route(
            "/api/v1/assets",
            get(assets::list_assets).post(assets::create_asset),
        )
        .route(
            "/api/v1/assets/:asset_id/draft",
            get(assets::get_draft).patch(assets::update_draft),
        )
        .route(
            "/api/v1/assets/:asset_id/publish",
            post(assets::publish_revision),
        )
        .route(
            "/api/v1/assets/:asset_id/revisions",
            get(assets::list_revisions),
        )
        .route(
            "/api/v1/assets/:asset_id/revisions/:revision_id",
            get(assets::get_revision),
        )
        // Variables & Preview
        .route(
            "/api/v1/variables/preview",
            post(variables::preview_variables),
        )
        // Runs & Reports
        .route("/api/v1/runs", get(runs::list_runs).post(runs::trigger_run))
        .route("/api/v1/runs/:run_id", get(runs::get_run))
        .route("/api/v1/runs/:run_id/stats", get(runs::get_run_stats))
        .route("/api/v1/runs/:run_id/cancel", post(runs::cancel_run))
        .route(
            "/api/v1/runs/:run_id/rerun-failed",
            post(runs::rerun_failed),
        )
        .route(
            "/api/v1/runs/:run_id/comparison",
            get(runs::get_run_comparison),
        )
        .route(
            "/api/v1/runs/:run_id/exports/:format",
            get(runs::export_run),
        )
        .route(
            "/api/v1/runs/:run_id/events",
            get(events::run_events_stream),
        )
        // Environments, Connections & Secrets
        .route(
            "/api/v1/environments",
            get(connections::list_environments).post(connections::create_environment),
        )
        .route(
            "/api/v1/connections",
            get(connections::list_connections).post(connections::create_connection),
        )
        .route(
            "/api/v1/connections/:id",
            axum::routing::put(connections::update_connection),
        )
        .route(
            "/api/v1/connections/:id/test",
            post(connections::test_connection),
        )
        .route(
            "/api/v1/secrets",
            get(connections::list_secrets).post(connections::create_secret),
        )
        // OpenAPI Specifications
        .route(
            "/api/v1/specifications/openapi/validate",
            post(specifications::validate_openapi),
        )
        .route(
            "/api/v1/specifications/openapi/import",
            post(specifications::import_openapi),
        )
        .merge(portability::routes())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_session,
        ));

    public.merge(protected).with_state(state)
}
