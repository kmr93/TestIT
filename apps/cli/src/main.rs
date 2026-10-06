use clap::{Parser, Subcommand};
use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "testit")]
#[command(about = "TestIT CI Automation Command-Line Tool", version = "0.1.0")]
struct Cli {
    #[arg(short, long, default_value = "http://localhost:8080")]
    server: String,

    #[command(subcommand)]
    command: Commands,
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
    let client = reqwest::Client::new();

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
                eprintln!("❌ Failed to trigger run: {}", resp.text().await?);
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
            let mut final_status = "QUEUED".to_string();

            loop {
                tokio::time::sleep(Duration::from_millis(800)).await;
                let status_url = format!("{}/api/v1/runs/{}", cli.server, run_id);
                let status_resp = client.get(&status_url).send().await?;

                if status_resp.status().is_success() {
                    let run_val: Value = status_resp.json().await?;
                    if let Some(status) = run_val["run"]["status"].as_str() {
                        final_status = status.to_string();
                        if matches!(
                            final_status.as_str(),
                            "PASSED" | "FAILED" | "ERROR" | "CANCELED" | "INTERRUPTED"
                        ) {
                            break;
                        }
                    }
                }
            }

            println!("\n🏁 Final Run Status: {}", final_status);

            // Download JUnit report if requested
            if let Some(junit_path) = junit {
                let junit_url = format!("{}/api/v1/runs/{}/exports/junit", cli.server, run_id);
                let junit_resp = client.get(&junit_url).send().await?;
                if junit_resp.status().is_success() {
                    let xml_data = junit_resp.text().await?;
                    tokio::fs::write(&junit_path, xml_data).await?;
                    println!("📄 JUnit XML report saved to: {:?}", junit_path);
                }
            }

            // Download HTML report if requested
            if let Some(html_path) = html {
                let html_url = format!("{}/api/v1/runs/{}/exports/html", cli.server, run_id);
                let html_resp = client.get(&html_url).send().await?;
                if html_resp.status().is_success() {
                    let html_data = html_resp.text().await?;
                    tokio::fs::write(&html_path, html_data).await?;
                    println!("📄 HTML report saved to: {:?}", html_path);
                }
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
