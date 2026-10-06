use std::env;

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub port: u16,
    pub database_url: String,
    pub nats_url: String,
    pub master_key: [u8; 32],
    pub artifacts_dir: String,
    pub docker_worker_image: String,
    pub max_active_cases: usize,
    pub bootstrap_admin_email: Option<String>,
    pub bootstrap_admin_password: Option<String>,
    pub cookie_secure: bool,
    pub cors_allowed_origins: Vec<String>,
    pub worker_manager_url: Option<String>,
    pub worker_manager_token: Option<String>,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, anyhow::Error> {
        let port = env::var("PORT")
            .unwrap_or_else(|_| "8080".to_string())
            .parse::<u16>()?;

        let database_url = env::var("DATABASE_URL")
            .unwrap_or_else(|_| "sqlite://data/db/testit.sqlite?mode=rwc".to_string());

        let nats_url = env::var("NATS_URL").unwrap_or_else(|_| "nats://127.0.0.1:4222".to_string());

        let master_key_hex = env::var("MASTER_KEY_HEX").map_err(|_| {
            anyhow::anyhow!("MASTER_KEY_HEX must be set to a unique 64-character key")
        })?;

        let key_bytes = hex::decode(&master_key_hex)?;
        if key_bytes.len() != 32 {
            anyhow::bail!("MASTER_KEY_HEX must be exactly 32 bytes (64 hex characters)");
        }
        let mut master_key = [0u8; 32];
        master_key.copy_from_slice(&key_bytes);

        let artifacts_dir =
            env::var("ARTIFACTS_DIR").unwrap_or_else(|_| "./data/artifacts".to_string());

        let docker_worker_image =
            env::var("DOCKER_WORKER_IMAGE").unwrap_or_else(|_| "testit-worker:latest".to_string());

        let max_active_cases = env::var("MAX_ACTIVE_CASES")
            .unwrap_or_else(|_| "4".to_string())
            .parse::<usize>()
            .unwrap_or(4);

        let bootstrap_admin_email = env::var("BOOTSTRAP_ADMIN_EMAIL")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let bootstrap_admin_password = env::var("BOOTSTRAP_ADMIN_PASSWORD")
            .ok()
            .filter(|value| !value.is_empty());
        let cookie_secure = env::var("COOKIE_SECURE")
            .unwrap_or_else(|_| "true".to_string())
            .parse::<bool>()?;
        let cors_allowed_origins = env::var("CORS_ALLOWED_ORIGINS")
            .unwrap_or_else(|_| "http://localhost:3000,http://127.0.0.1:3000".to_string())
            .split(',')
            .map(str::trim)
            .filter(|origin| !origin.is_empty())
            .map(ToOwned::to_owned)
            .collect();
        let worker_manager_url = env::var("WORKER_MANAGER_URL")
            .ok()
            .map(|value| value.trim_end_matches('/').to_string())
            .filter(|value| !value.is_empty());
        let worker_manager_token = env::var("WORKER_MANAGER_TOKEN")
            .ok()
            .filter(|value| !value.trim().is_empty());
        if worker_manager_url.is_some()
            && worker_manager_token
                .as_ref()
                .is_none_or(|token| token.len() < 64)
        {
            anyhow::bail!("WORKER_MANAGER_TOKEN must be a unique 64-character value when WORKER_MANAGER_URL is configured");
        }

        Ok(Self {
            port,
            database_url,
            nats_url,
            master_key,
            artifacts_dir,
            docker_worker_image,
            max_active_cases,
            bootstrap_admin_email,
            bootstrap_admin_password,
            cookie_secure,
            cors_allowed_origins,
            worker_manager_url,
            worker_manager_token,
        })
    }
}
