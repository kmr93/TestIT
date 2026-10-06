mod api;
mod config;
mod crypto;
mod db;
mod models;
mod orchestrator;
mod variables;

use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::{info, warn};

use config::AppConfig;
use db::DbPool;
use orchestrator::Orchestrator;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub db: DbPool,
    pub nats: Option<async_nats::Client>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,testit_control_plane=debug".into()),
        )
        .init();

    info!("Starting TestIT Control Plane v0.1.0...");

    // 2. Load configuration
    let config = Arc::new(AppConfig::from_env()?);

    // 3. Connect to SQLite & run migrations
    let db = db::init_db(&config.database_url).await?;

    // Seed default workspace, user, and environment if empty
    seed_bootstrap_data(&db).await?;

    // 4. Connect to NATS Core (graceful fallback if offline)
    let nats_client = match async_nats::connect(&config.nats_url).await {
        Ok(client) => {
            info!("Connected to private NATS Core server at {}", config.nats_url);
            Some(client)
        }
        Err(e) => {
            warn!(
                "NATS server unavailable at {} ({}). Operating in fallback polling mode; durable events remain preserved in SQLite.",
                config.nats_url, e
            );
            None
        }
    };

    let app_state = AppState {
        config: config.clone(),
        db: db.clone(),
        nats: nats_client,
    };

    // 5. Spawn background orchestrator and outbox drain loops
    let orchestrator = Arc::new(Orchestrator::new(app_state.clone()));
    tokio::spawn(orchestrator.run_loop());
    tokio::spawn(Orchestrator::outbox_drain_loop(app_state.clone()));

    // 6. Build Axum router
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let router = api::create_router(app_state)
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    info!("TestIT Control Plane listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router).await?;

    Ok(())
}

async fn seed_bootstrap_data(db: &DbPool) -> Result<(), anyhow::Error> {
    let now = chrono::Utc::now().to_rfc3339();

    // Default workspace
    let _ = sqlx::query(
        "INSERT OR IGNORE INTO workspaces (id, name, created_at)
         VALUES ('00000000-0000-0000-0000-000000000001', 'Default Workspace', ?)"
    )
    .bind(&now)
    .execute(db)
    .await?;

    // Default admin user
    let _ = sqlx::query(
        "INSERT OR IGNORE INTO users (id, workspace_id, email, display_name, role, password_hash, created_at)
         VALUES ('00000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000001', 'admin@testit.local', 'Admin User', 'ADMIN', 'hashed', ?)"
    )
    .bind(&now)
    .execute(db)
    .await?;

    // Default development environment
    let _ = sqlx::query(
        "INSERT OR IGNORE INTO environments (id, workspace_id, name, description, variables_json, created_at)
         VALUES ('00000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000001', 'Development', 'Local development environment', '{\"api_base_url\":\"https://httpbin.org\",\"timeout_ms\":5000}', ?)"
    )
    .bind(&now)
    .execute(db)
    .await?;

    Ok(())
}
