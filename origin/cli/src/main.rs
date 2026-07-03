use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod state;

#[derive(Parser)]
#[command(
    name = "tpt",
    about = "TPT Boxcar — Unified local sandbox for OCI + Wasm",
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
        .with_writer(std::io::stderr)
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

const MANIFEST_TEMPLATE: &str =
    include_str!("../../examples/getting-started/manifest.yaml");

async fn cmd_init(dir: &PathBuf) -> anyhow::Result<()> {
    let manifest_path = dir.join("manifest.yaml");
    if manifest_path.exists() {
        anyhow::bail!("manifest.yaml already exists in {}", dir.display());
    }

    std::fs::write(&manifest_path, MANIFEST_TEMPLATE)?;
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
            tpt_origin_core::manifest::Service::Process(process) => {
                println!("  {name}: Process ({})", process.command.join(" "));
            }
        }
    }

    let mut lifecycle = tpt_origin_core::lifecycle::LifecycleManager::new(&manifest);
    lifecycle.up(&manifest).await?;

    let state_dir = manifest_path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let pids = lifecycle.service_pids();
    let services = manifest
        .services
        .iter()
        .map(|(name, service)| state::ServiceState {
            name: name.clone(),
            service_type: match service {
                tpt_origin_core::manifest::Service::OCI(_) => "oci".to_string(),
                tpt_origin_core::manifest::Service::Wasm(_) => "wasm".to_string(),
                tpt_origin_core::manifest::Service::Process(_) => "process".to_string(),
            },
            pid: pids.get(name).copied(),
        })
        .collect();
    state::write(
        state_dir,
        &state::EnvironmentState {
            manifest_name: manifest.name.clone(),
            pid: std::process::id(),
            started_at: chrono_now(),
            services,
        },
    )?;

    println!("\nAll services are running. Press Ctrl+C to stop.");
    lifecycle.wait_for_signal().await?;
    lifecycle.down().await?;
    state::clear(state_dir)?;
    Ok(())
}

fn chrono_now() -> String {
    // Avoid pulling in a chrono dependency just for a timestamp string.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", now.as_secs())
}

async fn cmd_down() -> anyhow::Result<()> {
    let dir = std::path::Path::new(".");
    match state::read(dir)? {
        Some(env) => {
            println!("Tearing down environment: {}", env.manifest_name);
            for svc in &env.services {
                println!("  stopping {} ({})", svc.name, svc.service_type);
                // Kill real child processes directly first: force-killing the
                // parent `up` process below does not cascade to its children
                // (no process-group semantics on Windows), so without this a
                // `type: process` service would be orphaned rather than
                // actually torn down.
                if let Some(pid) = svc.pid {
                    if !state::terminate(pid) {
                        println!("    warning: could not signal process {pid} (it may have already exited)");
                    }
                }
            }
            if !state::terminate(env.pid) {
                println!(
                    "  warning: could not signal process {} (it may have already exited)",
                    env.pid
                );
            }
            state::clear(dir)?;
            println!("Environment torn down.");
        }
        None => println!("No running environment found (run `tpt origin up` first)."),
    }
    Ok(())
}

async fn cmd_ps() -> anyhow::Result<()> {
    println!("{:<20} {:<10} {:<10}", "NAME", "TYPE", "STATUS");
    println!("{:<20} {:<10} {:<10}", "----", "----", "------");
    if let Some(env) = state::read(std::path::Path::new("."))? {
        for svc in &env.services {
            println!("{:<20} {:<10} {:<10}", svc.name, svc.service_type, "running");
        }
    }
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
