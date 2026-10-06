use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Pool, Sqlite};
use std::str::FromStr;
use std::time::Duration;
use tracing::info;

pub type DbPool = Pool<Sqlite>;

pub async fn init_db(database_url: &str) -> Result<DbPool, anyhow::Error> {
    // Ensure parent directory exists if using a file path
    if let Some(path_str) = database_url.strip_prefix("sqlite://") {
        let clean_path = path_str.split('?').next().unwrap_or(path_str);
        if let Some(parent) = std::path::Path::new(clean_path).parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent).await?;
            }
        }
    }

    let connection_options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));

    let pool = SqlitePoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .connect_with(connection_options)
        .await?;

    info!("SQLite connection established in WAL mode with foreign keys enabled");

    // Execute schema migrations
    run_migrations(&pool).await?;

    Ok(pool)
}

pub async fn run_migrations(pool: &DbPool) -> Result<(), anyhow::Error> {
    info!("Verifying and applying SQLite schema migrations...");

    // We embed the primary migration SQL directly to guarantee bulletproof deployment
    let migration_sql = include_str!("../../../migrations/0001_initial_schema.sql");

    // Execute within a transaction or raw batch
    sqlx::raw_sql(migration_sql).execute(pool).await?;

    let auth_migration = include_str!("../../../migrations/0002_auth_rate_limit.sql");
    sqlx::raw_sql(auth_migration).execute(pool).await?;

    let users_migration = include_str!("../../../migrations/0003_disabled_users.sql");
    sqlx::raw_sql(users_migration).execute(pool).await?;

    info!("Database migrations applied successfully");
    Ok(())
}
