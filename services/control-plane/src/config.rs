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
}

impl AppConfig {
    pub fn from_env() -> Result<Self, anyhow::Error> {
        let port = env::var("PORT")
            .unwrap_or_else(|_| "8080".to_string())
            .parse::<u16>()?;

        let database_url = env::var("DATABASE_URL")
            .unwrap_or_else(|_| "sqlite://data/db/testit.sqlite?mode=rwc".to_string());

        let nats_url = env::var("NATS_URL")
            .unwrap_or_else(|_| "nats://127.0.0.1:4222".to_string());

        let master_key_hex = env::var("MASTER_KEY_HEX").unwrap_or_else(|_| {
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string()
        });

        let key_bytes = hex::decode(&master_key_hex)?;
        if key_bytes.len() != 32 {
            anyhow::bail!("MASTER_KEY_HEX must be exactly 32 bytes (64 hex characters)");
        }
        let mut master_key = [0u8; 32];
        master_key.copy_from_slice(&key_bytes);

        let artifacts_dir = env::var("ARTIFACTS_DIR")
            .unwrap_or_else(|_| "./data/artifacts".to_string());

        let docker_worker_image = env::var("DOCKER_WORKER_IMAGE")
            .unwrap_or_else(|_| "testit-worker:latest".to_string());

        let max_active_cases = env::var("MAX_ACTIVE_CASES")
            .unwrap_or_else(|_| "4".to_string())
            .parse::<usize>()
            .unwrap_or(4);

        Ok(Self {
            port,
            database_url,
            nats_url,
            master_key,
            artifacts_dir,
            docker_worker_image,
            max_active_cases,
        })
    }
}
