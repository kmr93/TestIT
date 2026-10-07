mod api;
mod config;
mod crypto;
mod db;
mod models;
mod orchestrator;
mod resource_locks;
mod variables;

use axum::http::{header, HeaderName, HeaderValue, Method};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
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
    seed_bootstrap_data(&db, &config).await?;

    // 4. Connect to NATS Core (graceful fallback if offline)
    let nats_connection = match (
        config.nats_control_user.as_ref(),
        config.nats_control_password.as_ref(),
    ) {
        (Some(user), Some(password)) => {
            async_nats::ConnectOptions::with_user_and_password(user.clone(), password.clone())
                .connect(&config.nats_url)
                .await
        }
        _ => async_nats::connect(&config.nats_url).await,
    };
    let nats_client = match nats_connection {
        Ok(client) => {
            info!("Connected to private NATS Core server");
            Some(client)
        }
        Err(e) => {
            warn!(
                "NATS server unavailable ({}). Operating in fallback polling mode; durable events remain preserved in SQLite.",
                e
            );
            None
        }
    };

    let app_state = AppState {
        config: config.clone(),
        db: db.clone(),
        nats: nats_client,
    };

    Orchestrator::recover_interrupted_runs(&app_state).await?;

    // 5. Spawn background orchestrator and outbox drain loops
    let orchestrator = Arc::new(Orchestrator::new(app_state.clone()));
    tokio::spawn(orchestrator.run_loop());
    tokio::spawn(Orchestrator::outbox_drain_loop(app_state.clone()));

    // 6. Build Axum router
    let cors_origins: Vec<HeaderValue> = config
        .cors_allowed_origins
        .iter()
        .filter_map(|origin| HeaderValue::from_str(origin).ok())
        .collect();
    let cors = CorsLayer::new()
        .allow_origin(cors_origins)
        .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::OPTIONS])
        .allow_headers([
            header::CONTENT_TYPE,
            HeaderName::from_static("x-csrf-token"),
        ])
        .allow_credentials(true);

    let router = api::create_router(app_state)
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    info!("TestIT Control Plane listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}

async fn seed_bootstrap_data(db: &DbPool, config: &AppConfig) -> Result<(), anyhow::Error> {
    let now = chrono::Utc::now().to_rfc3339();

    // Default workspace
    let _ = sqlx::query(
        "INSERT OR IGNORE INTO workspaces (id, name, created_at)
         VALUES ('00000000-0000-0000-0000-000000000001', 'Default Workspace', ?)",
    )
    .bind(&now)
    .execute(db)
    .await?;

    let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(db)
        .await?;
    let bootstrap = config
        .bootstrap_admin_email
        .as_deref()
        .zip(config.bootstrap_admin_password.as_deref());
    if let Some((email, password)) = bootstrap {
        if !email.contains('@') || password.chars().count() < 12 {
            anyhow::bail!("BOOTSTRAP_ADMIN_EMAIL must be an email and BOOTSTRAP_ADMIN_PASSWORD must have at least 12 characters");
        }
        let password_hash = api::auth::hash_password(password)?;
        if user_count == 0 {
            sqlx::query(
                "INSERT INTO users (id, workspace_id, email, display_name, role, password_hash, created_at)
                 VALUES (?, '00000000-0000-0000-0000-000000000001', ?, ?, 'ADMIN', ?, ?)",
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(email.trim().to_ascii_lowercase())
            .bind(email.split('@').next().unwrap_or("Administrator"))
            .bind(password_hash)
            .bind(&now)
            .execute(db)
            .await?;
        } else {
            // Migrate the insecure placeholder account created by earlier builds only
            // when an operator explicitly supplies replacement bootstrap credentials.
            sqlx::query(
                "UPDATE users SET email = ?, display_name = ?, password_hash = ?
                 WHERE email = 'admin@testit.local' AND password_hash = 'hashed'",
            )
            .bind(email.trim().to_ascii_lowercase())
            .bind(email.split('@').next().unwrap_or("Administrator"))
            .bind(password_hash)
            .execute(db)
            .await?;
        }
    } else if user_count == 0 {
        anyhow::bail!("No user exists yet. Set BOOTSTRAP_ADMIN_EMAIL and a 12+ character BOOTSTRAP_ADMIN_PASSWORD to create the first administrator.");
    }

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
