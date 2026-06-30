use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "tpt",
    about = "TPT Cloud-Native — Unified local sandbox for OCI + Wasm",
    version,
    long_about = "TPT Origin spins up a mix of traditional containers and Wasm microservices from a single manifest, with zero-config networking."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Origin {
        #[command(subcommand)]
        command: OriginCommands,
    },
}

#[derive(Subcommand)]
enum OriginCommands {
    /// Scaffold a new manifest.yaml in the current directory
    Init {
        /// Directory to scaffold in
        #[arg(short, long, default_value = ".")]
        dir: PathBuf,
    },
    /// Start all services defined in the manifest
    Up {
        /// Path to manifest file
        #[arg(short, long, default_value = "manifest.yaml")]
        manifest: PathBuf,
    },
    /// Tear down all running services
    Down,
    /// List running services with status
    Ps,
    /// Stream logs for a specific service
    Logs {
        /// Service name
        service: String,
        /// Follow log stream
        #[arg(short, long)]
        follow: bool,
    },
    /// Execute a command inside a running container or Wasm instance
    Exec {
        /// Service name
        service: String,
        /// Command to execute
        cmd: Vec<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Origin { command } => match command {
            OriginCommands::Init { dir } => cmd_init(&dir).await,
            OriginCommands::Up { manifest } => cmd_up(&manifest).await,
            OriginCommands::Down => cmd_down().await,
            OriginCommands::Ps => cmd_ps().await,
            OriginCommands::Logs { service, follow } => cmd_logs(&service, follow).await,
            OriginCommands::Exec { service, cmd } => cmd_exec(&service, &cmd).await,
        },
    }
}

async fn cmd_init(dir: &PathBuf) -> anyhow::Result<()> {
    let manifest_path = dir.join("manifest.yaml");
    if manifest_path.exists() {
        anyhow::bail!("manifest.yaml already exists in {}", dir.display());
    }

    let template = r#"# TPT Origin Manifest
# Docs: https://github.com/tpt-cloud-native/tpt-cloud-native/blob/main/docs/origin/manifest.md

name: my-app
version: "1.0"

services:
  # Example OCI container
  db:
    type: oci
    image: postgres:16
    ports:
      - host: 5432
        container: 5432
    environment:
      POSTGRES_PASSWORD: CHANGE_ME  # WARNING: replace with a strong password before use
    volumes:
      - source: pgdata
        target: /var/lib/postgresql/data

  # Example Wasm microservice
  api:
    type: wasm
    path: ./target/api.wasm
    environment:
      DB_HOST: db
      DB_PORT: "5432"
    depends_on:
      - db

networks:
  default:
    driver: bridge

volumes:
  pgdata: {}
"#;

    std::fs::write(&manifest_path, template)?;
    println!("Created manifest.yaml in {}", dir.display());
    Ok(())
}

async fn cmd_up(manifest_path: &PathBuf) -> anyhow::Result<()> {
    if !manifest_path.exists() {
        anyhow::bail!("Manifest not found: {}", manifest_path.display());
    }

    let content = std::fs::read_to_string(manifest_path)?;
    let manifest: tpt_origin_core::manifest::Manifest = serde_yaml::from_str(&content)?;

    println!("Bringing up environment: {}", manifest.name);
    println!("Services:");
    for (name, service) in &manifest.services {
        match service {
            tpt_origin_core::manifest::Service::OCI(oci) => {
                println!("  {name}: OCI ({})", oci.image);
            }
            tpt_origin_core::manifest::Service::Wasm(wasm) => {
                println!("  {name}: Wasm ({})", wasm.path.display());
            }
        }
    }

    let mut lifecycle = tpt_origin_core::lifecycle::LifecycleManager::new(&manifest);
    lifecycle.up(&manifest).await?;

    println!("\nAll services are running. Press Ctrl+C to stop.");
    lifecycle.wait_for_signal().await?;
    lifecycle.down().await?;
    Ok(())
}

async fn cmd_down() -> anyhow::Result<()> {
    println!("Tearing down services...");
    Ok(())
}

async fn cmd_ps() -> anyhow::Result<()> {
    println!("{:<20} {:<10} {:<10}", "NAME", "TYPE", "STATUS");
    println!("{:<20} {:<10} {:<10}", "----", "----", "------");
    Ok(())
}

async fn cmd_logs(service: &str, follow: bool) -> anyhow::Result<()> {
    println!("Streaming logs for service: {service} (follow={follow})");
    Ok(())
}

async fn cmd_exec(service: &str, cmd: &[String]) -> anyhow::Result<()> {
    println!("Executing {:?} in {service}", cmd);
    Ok(())
}
