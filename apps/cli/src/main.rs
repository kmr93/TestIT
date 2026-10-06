use clap::{Parser, Subcommand};
use reqwest::{header, Client, RequestBuilder};
use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "testit")]
#[command(about = "TestIT CI Automation Command-Line Tool", version = "0.1.0")]
struct Cli {
    #[arg(short, long, default_value = "http://localhost:8080")]
    server: String,

    #[arg(long, env = "TESTIT_EMAIL")]
    email: Option<String>,

    #[arg(long, default_value_t = 3600)]
    timeout_seconds: u64,

    #[command(subcommand)]
    command: Commands,
}

struct AuthenticatedClient {
    client: Client,
    cookie: String,
    csrf: String,
}

impl AuthenticatedClient {
    fn get(&self, url: &str) -> RequestBuilder {
        self.client.get(url).header(header::COOKIE, &self.cookie)
    }

    fn post(&self, url: &str) -> RequestBuilder {
        self.client
            .post(url)
            .header(header::COOKIE, &self.cookie)
            .header("x-csrf-token", &self.csrf)
    }
}

#[derive(Subcommand)]
enum Commands {
    /// Trigger a test suite run and wait for results
    Run {
        #[arg(short, long)]
        suite: String,

        #[arg(short, long)]
        env: String,

        #[arg(long, default_value_t = true)]
        wait: bool,

        #[arg(long)]
        junit: Option<PathBuf>,

        #[arg(long)]
        html: Option<PathBuf>,
    },
    /// Query status and stats of an existing run
    Status {
        #[arg(short, long)]
        run: String,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()?;
    let email = cli
        .email
        .or_else(|| std::env::var("TESTIT_EMAIL").ok())
        .ok_or("Set TESTIT_EMAIL or pass --email")?;
    let password = std::env::var("TESTIT_PASSWORD")
        .map_err(|_| "Set TESTIT_PASSWORD in the CI secret environment")?;
    let client = authenticate(client, &cli.server, &email, &password).await?;

    match cli.command {
        Commands::Run {
            suite,
            env,
            wait,
            junit,
            html,
        } => {
            println!("🚀 Triggering TestIT Suite: {}", suite);
            let trigger_url = format!("{}/api/v1/runs", cli.server);
            let payload = serde_json::json!({
                "suite_revision_id": suite,
                "environment_id": env,
                "inputs": {},
                "idempotency_key": uuid::Uuid::new_v4().to_string()
            });

            let resp = client.post(&trigger_url).json(&payload).send().await?;
            if !resp.status().is_success() {
                eprintln!("❌ Failed to trigger run (HTTP {})", resp.status());
                std::process::exit(2);
            }

            let body: Value = resp.json().await?;
            let run_id = body["run_id"].as_str().unwrap_or("unknown");
            println!("✅ Run initiated successfully. Run ID: {}", run_id);

            if !wait {
                println!("Run is queued. Exiting without waiting.");
                return Ok(());
            }

            println!("⏳ Waiting for execution to complete...");
            let deadline =
                tokio::time::Instant::now() + Duration::from_secs(cli.timeout_seconds.max(1));
            let final_status = loop {
                if tokio::time::Instant::now() >= deadline {
                    return Err(format!(
                        "Run {run_id} did not finish before the {} second deadline",
                        cli.timeout_seconds
                    )
                    .into());
                }
                tokio::time::sleep(Duration::from_millis(800)).await;
                let status_url = format!("{}/api/v1/runs/{}", cli.server, run_id);
                let status_resp = client.get(&status_url).send().await?;
                if !status_resp.status().is_success() {
                    return Err(format!(
                        "Run status request failed (HTTP {})",
                        status_resp.status()
                    )
                    .into());
                }
                let run_val: Value = status_resp.json().await?;
                let status = run_val["run"]["status"]
                    .as_str()
                    .ok_or("Run status response did not include a status")?;
                if matches!(
                    status,
                    "PASSED" | "FAILED" | "ERROR" | "CANCELED" | "INTERRUPTED"
                ) {
                    break status.to_string();
                }
            };

            println!("\n🏁 Final Run Status: {}", final_status);

            // Download JUnit report if requested
            if let Some(junit_path) = junit {
                let junit_url = format!("{}/api/v1/runs/{}/exports/junit", cli.server, run_id);
                let junit_resp = client.get(&junit_url).send().await?;
                if !junit_resp.status().is_success() {
                    return Err(format!(
                        "JUnit report download failed (HTTP {})",
                        junit_resp.status()
                    )
                    .into());
                }
                let xml_data = junit_resp.text().await?;
                tokio::fs::write(&junit_path, xml_data).await?;
                println!("📄 JUnit XML report saved to: {:?}", junit_path);
            }

            // Download HTML report if requested
            if let Some(html_path) = html {
                let html_url = format!("{}/api/v1/runs/{}/exports/html", cli.server, run_id);
                let html_resp = client.get(&html_url).send().await?;
                if !html_resp.status().is_success() {
                    return Err(format!(
                        "HTML report download failed (HTTP {})",
                        html_resp.status()
                    )
                    .into());
                }
                let html_data = html_resp.text().await?;
                tokio::fs::write(&html_path, html_data).await?;
                println!("📄 HTML report saved to: {:?}", html_path);
            }

            if final_status == "PASSED" {
                std::process::exit(0);
            } else if final_status == "FAILED" {
                std::process::exit(1);
            } else {
                std::process::exit(2);
            }
        }
        Commands::Status { run } => {
            let status_url = format!("{}/api/v1/runs/{}/stats", cli.server, run);
            let resp = client.get(&status_url).send().await?;
            if resp.status().is_success() {
                let stats: Value = resp.json().await?;
                println!("{}", serde_json::to_string_pretty(&stats)?);
            } else {
                eprintln!("Failed to fetch stats: {}", resp.status());
                std::process::exit(1);
            }
        }
    }

    Ok(())
}

async fn authenticate(
    client: Client,
    server: &str,
    email: &str,
    password: &str,
) -> Result<AuthenticatedClient, Box<dyn std::error::Error>> {
    let response = client
        .post(format!(
            "{}/api/v1/auth/login",
            server.trim_end_matches('/')
        ))
        .json(&serde_json::json!({ "email": email, "password": password }))
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(format!("Authentication failed (HTTP {})", response.status()).into());
    }
    let mut cookie_parts = Vec::new();
    let mut csrf = None;
    for value in response.headers().get_all(header::SET_COOKIE) {
        let item = value.to_str()?;
        if let Some(pair) = item.split(';').next() {
            if pair.starts_with("testit_session=") {
                cookie_parts.push(pair.to_string());
            } else if let Some(token) = pair.strip_prefix("testit_csrf=") {
                cookie_parts.push(pair.to_string());
                csrf = Some(token.to_string());
            }
        }
    }
    let csrf = csrf.ok_or("Login response omitted the CSRF cookie")?;
    if !cookie_parts
        .iter()
        .any(|part| part.starts_with("testit_session="))
    {
        return Err("Login response omitted the session cookie".into());
    }
    Ok(AuthenticatedClient {
        client,
        cookie: cookie_parts.join("; "),
        csrf,
    })
}
