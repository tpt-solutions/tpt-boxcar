use anyhow::Context;
use clap::{Parser, Subcommand};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
    /// Bring up a cross-product environment (Tether + Origin + Frontier, or
    /// any subset) from a single `boxcar.yaml` manifest. Services are
    /// started in `depends_on` order via the same `LifecycleManager` that
    /// backs `tpt origin up` — Tether/Frontier's control planes run as
    /// `type: process` entries alongside native `oci`/`wasm` Origin
    /// services in the same manifest.
    Up {
        /// Path to the cross-product manifest file
        #[arg(short, long, default_value = "boxcar.yaml")]
        manifest: PathBuf,
    },
    /// Tear down a running `tpt up` environment
    Down,
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
        /// Watch source directories for changes and restart affected services.
        /// Monitors directories containing service source files (wasm modules,
        /// process binaries, Dockerfiles) and triggers a service restart on
        /// change. Critical for developer iteration speed.
        #[arg(long)]
        watch: bool,
        /// Run in rootless mode: uses user namespaces and slirp4netns for
        /// networking so `tpt origin up` works without root privileges.
        /// Requires rootless containerd (socket at
        /// $XDG_RUNTIME_DIR/containerd/containerd.sock) and slirp4netns to
        /// be installed.
        #[arg(long)]
        rootless: bool,
        /// Profiles to activate (comma-separated). Only services with matching
        /// profiles (or no profile restriction) will be started.
        #[arg(short, long)]
        profiles: Option<String>,
        /// Environment variable overrides (KEY=VALUE). Can be specified
        /// multiple times. Used for variable interpolation in manifests.
        #[arg(short = 'E', long = "env")]
        env_overrides: Vec<String>,
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
    /// Build an OCI image from a Dockerfile using containerd
    Build {
        /// Path to Dockerfile (default: ./Dockerfile)
        #[arg(short = 'f', long = "file", default_value = "Dockerfile")]
        dockerfile: PathBuf,
        /// Build context directory (default: directory containing Dockerfile)
        #[arg(short = 'c', long = "context")]
        context: Option<PathBuf>,
        /// Image tag (e.g. "myapp:latest")
        #[arg(short = 't', long = "tag", required = true)]
        tag: String,
        /// Build argument (KEY=VALUE), can be specified multiple times
        #[arg(short = 'B', long = "build-arg")]
        build_args: Vec<String>,
    },
    /// Execute a command inside a running container or Wasm instance
    Exec {
        /// Service name
        service: String,
        /// Command to execute
        cmd: Vec<String>,
    },
    /// Re-run a captured Wasm invocation offline, using the exact args/env
    /// recorded by Scope at the time it ran (deterministic replay /
    /// time-travel debugging).
    Replay {
        /// Path to manifest file (used to locate the service's wasm module)
        #[arg(short, long, default_value = "manifest.yaml")]
        manifest: PathBuf,
        /// Service name (must be a `type: wasm` service in the manifest)
        service: String,
        /// Base URL of the Scope query API (e.g. http://localhost:8081)
        #[arg(long, default_value = "http://localhost:8081")]
        scope_url: String,
    },
    /// Show detailed configuration and runtime state for a service
    Inspect {
        /// Service name to inspect
        service: String,
        /// Path to manifest file
        #[arg(short, long, default_value = "manifest.yaml")]
        manifest: PathBuf,
        /// Output as JSON instead of human-readable format
        #[arg(long)]
        json: bool,
    },
    /// Show live CPU/memory/network usage per service
    Stats {
        /// Path to manifest file
        #[arg(short, long, default_value = "manifest.yaml")]
        manifest: PathBuf,
        /// Continuously refresh stats (like `docker stats`)
        #[arg(short, long)]
        follow: bool,
    },
    /// List locally stored OCI images
    Images {
        /// Filter by image name or reference
        #[arg(long)]
        filter: Option<String>,
    },
    /// Remove a locally stored OCI image
    Rmi {
        /// Image reference to remove
        image: String,
    },
    /// Pull an OCI image from a registry
    Pull {
        /// Image reference to pull (e.g. "postgres:16")
        image: String,
    },
    /// Push a built OCI image to a registry
    Push {
        /// Image reference to push (e.g. "ghcr.io/myorg/myimage:latest")
        image: String,
        /// Path to Docker config.json for registry credentials
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Show real-time lifecycle events (like `docker events`)
    Events {
        /// Path to manifest file
        #[arg(short, long, default_value = "manifest.yaml")]
        manifest: PathBuf,
        /// Filter by event type (e.g. "start", "stop", "health")
        #[arg(short, long)]
        filter: Option<String>,
        /// Filter by service name
        #[arg(short, long)]
        service: Option<String>,
        /// Output as JSON instead of human-readable format
        #[arg(long)]
        json: bool,
    },
    /// Copy files to/from containers (like `docker cp`)
    Cp {
        /// Source path (container:path or host:path)
        source: String,
        /// Destination path (container:path or host:path)
        destination: String,
    },
    /// Pause a running service (freezes CPU and memory)
    Pause {
        /// Service name to pause
        service: String,
    },
    /// Unpause a paused service (resumes execution)
    Unpause {
        /// Service name to unpause
        service: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Origin { command } => match command {
            OriginCommands::Init { dir } => cmd_init(&dir).await,
            OriginCommands::Up {
                manifest,
                watch,
                rootless,
                profiles,
                env_overrides,
            } => {
                let profile_list = profiles
                    .as_deref()
                    .map(|p| {
                        p.split(',')
                            .map(|s| s.trim().to_string())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let env_map = env_overrides
                    .iter()
                    .filter_map(|e| {
                        e.split_once('=')
                            .map(|(k, v)| (k.to_string(), v.to_string()))
                    })
                    .collect();
                cmd_up(&manifest, watch, rootless, &profile_list, &env_map).await
            }
            OriginCommands::Down => cmd_down().await,
            OriginCommands::Ps => cmd_ps().await,
            OriginCommands::Logs { service, follow } => cmd_logs(&service, follow).await,
            OriginCommands::Exec { service, cmd } => cmd_exec(&service, &cmd).await,
            OriginCommands::Build {
                dockerfile,
                context,
                tag,
                build_args,
            } => cmd_build(&dockerfile, context.as_ref(), &tag, &build_args).await,
            OriginCommands::Replay {
                manifest,
                service,
                scope_url,
            } => cmd_replay(&manifest, &service, &scope_url).await,
            OriginCommands::Inspect {
                service,
                manifest,
                json,
            } => cmd_inspect(&service, &manifest, json).await,
            OriginCommands::Stats { manifest, follow } => cmd_stats(&manifest, follow).await,
            OriginCommands::Images { filter } => cmd_images(filter.as_deref()).await,
            OriginCommands::Rmi { image } => cmd_rmi(&image).await,
            OriginCommands::Pull { image } => cmd_pull(&image).await,
            OriginCommands::Push { image, config } => cmd_push(&image, config.as_ref()).await,
            OriginCommands::Events {
                manifest,
                filter,
                service,
                json,
            } => cmd_events(&manifest, filter.as_deref(), service.as_deref(), json).await,
            OriginCommands::Cp {
                source,
                destination,
            } => cmd_cp(&source, &destination).await,
            OriginCommands::Pause { service } => cmd_pause(&service).await,
            OriginCommands::Unpause { service } => cmd_unpause(&service).await,
        },
        Commands::Up { manifest } => {
            let empty_profiles = Vec::new();
            let empty_env = HashMap::new();
            cmd_up(&manifest, false, false, &empty_profiles, &empty_env).await
        }
        Commands::Down => cmd_down().await,
    }
}

const MANIFEST_TEMPLATE: &str = include_str!("../../examples/getting-started/manifest.yaml");

async fn cmd_init(dir: &Path) -> anyhow::Result<()> {
    let manifest_path = dir.join("manifest.yaml");
    if manifest_path.exists() {
        anyhow::bail!("manifest.yaml already exists in {}", dir.display());
    }

    std::fs::write(&manifest_path, MANIFEST_TEMPLATE)?;
    println!("Created manifest.yaml in {}", dir.display());
    Ok(())
}

async fn cmd_up(
    manifest_path: &PathBuf,
    watch: bool,
    rootless: bool,
    profiles: &[String],
    env_overrides: &HashMap<String, String>,
) -> anyhow::Result<()> {
    if !manifest_path.exists() {
        anyhow::bail!("Manifest not found: {}", manifest_path.display());
    }

    let content = std::fs::read_to_string(manifest_path)?;
    let mut manifest: tpt_origin_core::manifest::Manifest = serde_yaml::from_str(&content)?;

    // Resolve extends
    manifest = tpt_origin_core::manifest::resolve_extends(&manifest)
        .map_err(|e| anyhow::anyhow!("failed to resolve extends: {e}"))?;

    // Filter by profiles
    if !profiles.is_empty() {
        manifest = tpt_origin_core::manifest::filter_by_profiles(&manifest, profiles);
    }

    // Interpolate environment variables in service configurations
    let system_env: HashMap<String, String> = std::env::vars().collect();
    for service in manifest.services.values_mut() {
        interpolate_service_env(service, &system_env, env_overrides);
    }

    if rootless {
        println!("Running in rootless mode");
        // Point containerd client to the rootless socket if not already set
        if std::env::var("ORIGIN_CONTAINERD_SOCKET").is_err() {
            let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| {
                #[cfg(unix)]
                {
                    format!("/run/user/{}", unsafe { libc::getuid() })
                }
                #[cfg(not(unix))]
                {
                    "/tmp/runtime".to_string()
                }
            });
            let rootless_socket = format!("{runtime_dir}/containerd/containerd.sock");
            std::env::set_var("ORIGIN_CONTAINERD_SOCKET", &rootless_socket);
        }
    }

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

    let mut origin = if rootless {
        tpt_origin_core::Origin::new_rootless(&manifest)
    } else {
        tpt_origin_core::Origin::new(&manifest)
    };
    origin.up(&manifest).await?;

    let state_dir = manifest_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let pids = origin.service_pids();
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

    if watch {
        run_with_watch(&mut origin, &manifest, state_dir).await
    } else {
        run_supervisor(&mut origin, &manifest, state_dir).await
    }
}

/// Standard supervision loop without file watching.
async fn run_supervisor(
    origin: &mut tpt_origin_core::Origin,
    manifest: &tpt_origin_core::manifest::Manifest,
    state_dir: &std::path::Path,
) -> anyhow::Result<()> {
    let mut supervise_tick = tokio::time::interval(std::time::Duration::from_secs(5));
    supervise_tick.tick().await;
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("Received shutdown signal");
                break;
            }
            _ = supervise_tick.tick() => {
                if let Err(e) = origin.reap_and_restart(manifest).await {
                    tracing::warn!("reap_and_restart failed: {e}");
                }
                if let Err(e) = origin.poll_healthchecks(manifest).await {
                    tracing::warn!("poll_healthchecks failed: {e}");
                }
            }
        }
    }
    origin.down().await?;
    state::clear(state_dir)?;
    Ok(())
}

/// Supervision loop with file watching: monitors source directories for
/// changes and restarts affected services on file modification.
async fn run_with_watch(
    origin: &mut tpt_origin_core::Origin,
    manifest: &tpt_origin_core::manifest::Manifest,
    state_dir: &std::path::Path,
) -> anyhow::Result<()> {
    use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
    use tokio::sync::mpsc;

    // Collect watch paths from the manifest: directories containing
    // service source files (wasm module parents, process command dirs, etc.)
    let mut watch_dirs: Vec<std::path::PathBuf> = Vec::new();
    let manifest_dir = manifest_path_for(manifest);

    for service in manifest.services.values() {
        match service {
            tpt_origin_core::manifest::Service::Wasm(wasm) => {
                if let Some(parent) = wasm.path.parent() {
                    let dir = resolve_watch_dir(parent, &manifest_dir);
                    if !watch_dirs.contains(&dir) {
                        tracing::info!("watching wasm source dir: {}", dir.display());
                        watch_dirs.push(dir);
                    }
                }
            }
            tpt_origin_core::manifest::Service::Process(process) => {
                if let Some(program) = process.command.first() {
                    // Watch the directory containing the program binary/script
                    let path = std::path::Path::new(program);
                    if path.is_relative() {
                        let dir = resolve_watch_dir(
                            path.parent().unwrap_or(std::path::Path::new(".")),
                            &manifest_dir,
                        );
                        if !watch_dirs.contains(&dir) {
                            tracing::info!("watching process source dir: {}", dir.display());
                            watch_dirs.push(dir);
                        }
                    }
                }
            }
            _ => {} // OCI services use images, not local source
        }
    }

    if watch_dirs.is_empty() {
        tracing::warn!(
            "no watchable source directories found in manifest; --watch has nothing to monitor"
        );
        run_supervisor(origin, manifest, state_dir).await?;
        return Ok(());
    }

    println!("Watching {} directory for changes...", watch_dirs.len());
    for dir in &watch_dirs {
        println!("  - {}", dir.display());
    }
    println!("Press Ctrl+C to stop.\n");

    let (tx, mut rx) = mpsc::channel::<notify::Result<Event>>(64);
    let mut watcher: RecommendedWatcher = Watcher::new(
        move |res| {
            let _ = tx.blocking_send(res);
        },
        notify::Config::default().with_poll_interval(std::time::Duration::from_secs(2)),
    )?;

    for dir in &watch_dirs {
        watcher.watch(dir, RecursiveMode::Recursive)?;
    }

    let mut supervise_tick = tokio::time::interval(std::time::Duration::from_secs(5));
    supervise_tick.tick().await;
    // Debounce: skip restarts within 500ms of each other
    let mut last_restart = std::time::Instant::now() - std::time::Duration::from_secs(10);

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("Received shutdown signal");
                break;
            }
            _ = supervise_tick.tick() => {
                if let Err(e) = origin.reap_and_restart(manifest).await {
                    tracing::warn!("reap_and_restart failed: {e}");
                }
                if let Err(e) = origin.poll_healthchecks(manifest).await {
                    tracing::warn!("poll_healthchecks failed: {e}");
                }
            }
            event = rx.recv() => {
                if let Some(Ok(Event { kind: EventKind::Modify(_), paths, .. })) = event {
                    if last_restart.elapsed() < std::time::Duration::from_millis(500) {
                        continue; // debounce
                    }
                    // Find which services are affected by the changed files
                    let affected = find_affected_services(&paths, manifest, &manifest_dir);
                    if affected.is_empty() {
                        continue;
                    }
                    last_restart = std::time::Instant::now();
                    for service_name in &affected {
                        println!("\nDetected change in '{service_name}', restarting...");
                        if let Err(e) = origin.restart_service(service_name, manifest).await {
                            tracing::warn!("failed to restart '{service_name}': {e}");
                        } else {
                            println!("Restarted '{service_name}' successfully.");
                        }
                    }
                }
                // else: ignore other event types and errors
            }
        }
    }
    drop(watcher);
    origin.down().await?;
    state::clear(state_dir)?;
    Ok(())
}

/// Returns the manifest's parent directory, used as the base for resolving
/// relative paths in the manifest.
fn manifest_path_for(_manifest: &tpt_origin_core::manifest::Manifest) -> std::path::PathBuf {
    // The manifest path isn't stored on the Manifest struct, so we use cwd.
    // This is the same assumption the CLI already makes.
    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
}

/// Resolves a relative directory against the manifest base directory.
fn resolve_watch_dir(dir: &std::path::Path, base: &std::path::Path) -> std::path::PathBuf {
    if dir.is_absolute() {
        dir.to_path_buf()
    } else {
        base.join(dir)
    }
}

/// Given a set of changed file paths, returns the names of services that
/// are affected (i.e. the changed file is in a directory related to that
/// service's source).
fn find_affected_services(
    changed: &[std::path::PathBuf],
    manifest: &tpt_origin_core::manifest::Manifest,
    manifest_dir: &std::path::Path,
) -> Vec<String> {
    let mut affected = Vec::new();

    for (name, service) in &manifest.services {
        let is_affected = match service {
            tpt_origin_core::manifest::Service::Wasm(wasm) => {
                if let Some(parent) = wasm.path.parent() {
                    let service_dir = resolve_watch_dir(parent, manifest_dir);
                    changed.iter().any(|p| p.starts_with(&service_dir))
                } else {
                    false
                }
            }
            tpt_origin_core::manifest::Service::Process(process) => {
                if let Some(program) = process.command.first() {
                    let path = std::path::Path::new(program);
                    if path.is_relative() {
                        let service_dir = resolve_watch_dir(
                            path.parent().unwrap_or(std::path::Path::new(".")),
                            manifest_dir,
                        );
                        changed.iter().any(|p| p.starts_with(&service_dir))
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            _ => false,
        };
        if is_affected {
            affected.push(name.clone());
        }
    }

    affected
}

fn chrono_now() -> String {
    // Avoid pulling in a chrono dependency just for a timestamp string.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", now.as_secs())
}

/// Interpolates environment variables in a service's configuration fields.
fn interpolate_service_env(
    service: &mut tpt_origin_core::manifest::Service,
    env: &HashMap<String, String>,
    overrides: &HashMap<String, String>,
) {
    match service {
        tpt_origin_core::manifest::Service::OCI(oci) => {
            oci.image = tpt_origin_core::manifest::interpolate_env(&oci.image, env, overrides);
            for v in &mut oci.volumes {
                v.source = tpt_origin_core::manifest::interpolate_env(&v.source, env, overrides);
                v.target = tpt_origin_core::manifest::interpolate_env(&v.target, env, overrides);
            }
            for _port in &mut oci.ports {
                // Port mappings are numeric, skip interpolation
            }
        }
        tpt_origin_core::manifest::Service::Wasm(wasm) => {
            let path_str = wasm.path.to_string_lossy().to_string();
            let interpolated =
                tpt_origin_core::manifest::interpolate_env(&path_str, env, overrides);
            wasm.path = std::path::PathBuf::from(interpolated);
        }
        tpt_origin_core::manifest::Service::Process(process) => {
            for arg in &mut process.command {
                *arg = tpt_origin_core::manifest::interpolate_env(arg, env, overrides);
            }
        }
    }

    // Interpolate environment values themselves
    let env_map = service.environment_mut().clone();
    for (key, value) in env_map {
        let interpolated = tpt_origin_core::manifest::interpolate_env(&value, env, overrides);
        service.environment_mut().insert(key, interpolated);
    }
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
            println!(
                "{:<20} {:<10} {:<10}",
                svc.name, svc.service_type, "running"
            );
        }
    }
    Ok(())
}

/// Path containerd writes a running OCI service's captured combined
/// stdout/stderr to (wired up in `RuntimeManager::start_oci` via `ctr run
/// --log-uri file://...`). Mirrors `tpt_origin_core::containerd::log_path`,
/// duplicated here (rather than depending on that module directly) since
/// the `containerd` module only exists on the `target_os = "linux"` +
/// `containerd` feature combination, while the CLI itself builds
/// everywhere.
fn oci_log_path(service: &str) -> std::path::PathBuf {
    let dir =
        std::env::var("ORIGIN_LOGS_DIR").unwrap_or_else(|_| "/var/lib/tpt-boxcar/logs".to_string());
    std::path::PathBuf::from(dir).join(format!("{service}.log"))
}

async fn cmd_logs(service: &str, follow: bool) -> anyhow::Result<()> {
    let env = state::read(std::path::Path::new("."))?.ok_or_else(|| {
        anyhow::anyhow!("no running environment found (run `tpt origin up` first)")
    })?;
    let svc = env
        .services
        .iter()
        .find(|s| s.name == service)
        .ok_or_else(|| {
            anyhow::anyhow!("service '{service}' not found in the running environment")
        })?;

    // For OCI services, log capture is handled by containerd's `--log-uri`.
    // For process services with `logging.driver: file`, log capture is
    // handled by in-process LogRotation. For wasm services, output goes
    // directly to the terminal. All types now write to the same log path
    // when file-based logging is enabled.
    let path = oci_log_path(service);
    if !path.exists() {
        anyhow::bail!(
            "no captured log file found at {} yet; \
             for '{}' (type: {}), logs may go directly to the terminal",
            path.display(),
            service,
            svc.service_type
        );
    }

    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(&path)?;
    let mut buf = String::new();
    file.read_to_string(&mut buf)?;
    print!("{buf}");

    if follow {
        let mut pos = file.seek(SeekFrom::End(0))?;
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let metadata = std::fs::metadata(&path)?;
            if metadata.len() > pos {
                file.seek(SeekFrom::Start(pos))?;
                let mut chunk = String::new();
                file.read_to_string(&mut chunk)?;
                print!("{chunk}");
                pos = metadata.len();
            }
        }
    }

    Ok(())
}

async fn cmd_exec(service: &str, cmd: &[String]) -> anyhow::Result<()> {
    anyhow::ensure!(!cmd.is_empty(), "no command specified");

    let env = state::read(std::path::Path::new("."))?.ok_or_else(|| {
        anyhow::anyhow!("no running environment found (run `tpt origin up` first)")
    })?;
    let svc = env
        .services
        .iter()
        .find(|s| s.name == service)
        .ok_or_else(|| {
            anyhow::anyhow!("service '{service}' not found in the running environment")
        })?;

    match svc.service_type.as_str() {
        "oci" => exec_oci(service, cmd).await,
        "process" => exec_process(cmd),
        other => Err(anyhow::anyhow!(
            "exec is not supported for '{other}' services"
        )),
    }
}

/// Executes `cmd` inside a running OCI container via `ctr tasks exec --tty`.
/// The `--tty` flag tells containerd to allocate a PTY inside the container
/// and forward its I/O over gRPC — stdio is inherited from this process so
/// the user gets a real interactive session. The pattern mirrors
/// `exec_healthcheck` in `tpt-origin-core` but adds `--tty` and inherits
/// stdio instead of capturing output.
async fn exec_oci(container_id: &str, cmd: &[String]) -> anyhow::Result<()> {
    let socket = std::env::var("ORIGIN_CONTAINERD_SOCKET")
        .unwrap_or_else(|_| "/run/containerd/containerd.sock".to_string());
    let namespace = "tpt-boxcar";
    let exec_id = format!("exec-{}", std::process::id());

    let mut args: Vec<String> = vec![
        "--address".into(),
        socket,
        "--namespace".into(),
        namespace.into(),
        "tasks".into(),
        "exec".into(),
        "--exec-id".into(),
        exec_id,
        "--tty".into(),
        container_id.into(),
    ];
    args.extend(cmd.iter().cloned());

    let status = tokio::process::Command::new("ctr")
        .args(&args)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .await
        .context("failed to spawn `ctr tasks exec` (is containerd's `ctr` CLI installed?)")?;

    if !status.success() {
        anyhow::bail!("command exited with status {status}");
    }
    Ok(())
}

/// Executes `cmd` in a new process using a pseudo-terminal for interactive
/// I/O. The child inherits the environment and working directory of the
/// original `tpt origin up` session. Uses `portable-pty` for cross-platform
/// PTY allocation and `crossterm` for raw-mode terminal handling.
fn exec_process(cmd: &[String]) -> anyhow::Result<()> {
    use crossterm::event::{Event, KeyCode, KeyModifiers};
    use crossterm::terminal;
    use portable_pty::{native_pty_system, CommandBuilder, PtySize};

    let (program, args) = cmd
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("empty command"))?;

    let pty_system = native_pty_system();
    let pair = pty_system.openpty(PtySize::default())?;

    let mut pty_cmd = CommandBuilder::new(program);
    pty_cmd.args(args);
    // Inherit the parent's environment so the exec'd command sees the same
    // vars as the service's original `tpt origin up` session.
    for (key, value) in std::env::vars() {
        pty_cmd.env(key, value);
    }

    let mut child = pair.slave.spawn_command(pty_cmd)?;

    // The master's reader is not Clone, so move it into a dedicated thread
    // that sends PTY output to the main thread via a channel. The main
    // thread reads keyboard input (crossterm in raw mode) and writes it to
    // the master's writer.
    let (pty_out_tx, pty_out_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let master_reader = pair.master.try_clone_reader()?;
    let reader_thread = std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        let mut reader = master_reader;
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if pty_out_tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let mut master_writer = pair.master.take_writer()?;

    // Enter raw mode so every keystroke is forwarded immediately (no line
    // buffering, no echo — the child process owns the terminal).
    terminal::enable_raw_mode()?;
    // Restore the terminal on panic so the user's shell isn't left in a
    // broken state.
    std::panic::set_hook(Box::new(|info| {
        let _ = terminal::disable_raw_mode();
        eprintln!("{info}");
    }));

    let result = (|| -> anyhow::Result<u32> {
        loop {
            // Poll PTY output first (non-blocking channel recv), then
            // block on the next keyboard event.
            while let Ok(data) = pty_out_rx.try_recv() {
                std::io::Write::write_all(&mut std::io::stdout(), &data)?;
                std::io::Write::flush(&mut std::io::stdout())?;
            }

            if crossterm::event::poll(std::time::Duration::from_millis(10))? {
                if let Event::Key(key) = crossterm::event::read()? {
                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        master_writer.write_all(&[0x03])?; // SIGINT
                    } else if key.code == KeyCode::Char('d')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        master_writer.write_all(&[0x04])?; // EOF
                    } else if let KeyCode::Char(c) = key.code {
                        master_writer.write_all(&[c as u8])?;
                    } else if key.code == KeyCode::Enter {
                        master_writer.write_all(b"\r")?;
                    } else if key.code == KeyCode::Backspace {
                        master_writer.write_all(&[0x7f])?;
                    } else if let KeyCode::Esc = key.code {
                        master_writer.write_all(&[0x1b])?;
                    } else if let KeyCode::Tab = key.code {
                        master_writer.write_all(b"\t")?;
                    } else if let KeyCode::Up = key.code {
                        master_writer.write_all(b"\x1b[A")?;
                    } else if let KeyCode::Down = key.code {
                        master_writer.write_all(b"\x1b[B")?;
                    } else if let KeyCode::Right = key.code {
                        master_writer.write_all(b"\x1b[C")?;
                    } else if let KeyCode::Left = key.code {
                        master_writer.write_all(b"\x1b[D")?;
                    }
                    master_writer.flush()?;
                }
            }

            // If the child has exited and the PTY has no more data, stop.
            if let Ok(None) = child.try_wait() {
                // Still running; drain remaining output below.
            } else if pty_out_rx.try_recv().is_err() {
                break;
            }
        }

        // Drain any remaining PTY output after the child exits.
        while let Ok(data) = pty_out_rx.recv() {
            std::io::Write::write_all(&mut std::io::stdout(), &data)?;
        }
        std::io::Write::flush(&mut std::io::stdout())?;

        let status = child.wait()?;
        Ok(status.exit_code())
    })();

    // Always restore terminal state, even on error.
    let _ = terminal::disable_raw_mode();
    let _ = reader_thread.join();

    let code = result?;
    if code != 0 {
        anyhow::bail!("command exited with code {code}");
    }
    Ok(())
}

#[derive(serde::Deserialize)]
struct WasmInvocationResponse {
    #[serde(rename = "moduleName")]
    #[allow(dead_code)]
    module_name: String,
    function: String,
    args: Vec<String>,
    env: std::collections::HashMap<String, String>,
    #[serde(rename = "wasmSha256")]
    wasm_sha256: String,
}

/// Re-runs a previously captured Wasm invocation entirely offline: the
/// only network access is the one-time fetch of the historical args/env
/// from Scope's query API; the actual re-execution touches no Tether/prod
/// state at all, matching "replay it offline without prod access".
async fn cmd_replay(manifest_path: &PathBuf, service: &str, scope_url: &str) -> anyhow::Result<()> {
    let content = std::fs::read_to_string(manifest_path)?;
    let manifest: tpt_origin_core::manifest::Manifest = serde_yaml::from_str(&content)?;

    let wasm_service = match manifest.services.get(service) {
        Some(tpt_origin_core::manifest::Service::Wasm(w)) => w,
        Some(_) => anyhow::bail!("service '{service}' is not a wasm service"),
        None => anyhow::bail!("service '{service}' not found in manifest"),
    };

    let wasm_bytes = std::fs::read(&wasm_service.path)?;
    let sha256 = tpt_origin_core::runtime::sha256_hex(&wasm_bytes);

    let url = format!("{scope_url}/api/v1/wasm-invocations?module={service}&sha256={sha256}");
    println!("Fetching captured invocation from {url}");
    let invocation: WasmInvocationResponse = ureq::get(&url)
        .call()
        .map_err(|e| anyhow::anyhow!("failed to fetch captured invocation: {e}"))?
        .into_json()
        .map_err(|e| anyhow::anyhow!("failed to parse captured invocation: {e}"))?;

    if invocation.wasm_sha256 != sha256 {
        anyhow::bail!(
            "captured invocation's wasm digest ({}) does not match the module on disk ({}); \
             the module may have changed since capture",
            invocation.wasm_sha256,
            sha256
        );
    }

    println!(
        "Replaying '{service}' with captured args={:?}, calling `{}`",
        invocation.args, invocation.function
    );
    tpt_origin_core::runtime::instantiate_and_run(
        &wasm_bytes,
        &invocation.args,
        &invocation.env,
        None,
    )?;
    println!("Replay completed successfully.");
    Ok(())
}

async fn cmd_inspect(
    service: &str,
    manifest_path: &PathBuf,
    json_output: bool,
) -> anyhow::Result<()> {
    let content = std::fs::read_to_string(manifest_path)?;
    let manifest: tpt_origin_core::manifest::Manifest = serde_yaml::from_str(&content)?;

    let env = state::read(std::path::Path::new(".")).ok().flatten();

    // Build an Origin instance to inspect from, using the manifest.
    let origin = tpt_origin_core::Origin::new(&manifest);

    if let Some(info) = origin.inspect(service, &manifest) {
        if json_output {
            println!("{}", serde_json::to_string_pretty(&info)?);
        } else {
            print_inspect_info(&info);
        }
    } else if let Some(env) = env {
        // Service not in live state; try to show from state file + manifest only.
        if let Some(svc) = env.services.iter().find(|s| s.name == service) {
            println!("Service: {}", svc.name);
            println!("Type:    {}", svc.service_type);
            println!(
                "PID:     {}",
                svc.pid
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| "N/A".to_string())
            );
            println!();
            println!(
                "(Service is recorded in state but not currently live in this Origin instance)"
            );
        } else {
            anyhow::bail!("service '{service}' not found in running environment or manifest");
        }
    } else {
        anyhow::bail!(
            "service '{service}' not found in manifest. Run `tpt origin up` first to start services, \
             or check the manifest for the correct service name."
        );
    }

    Ok(())
}

fn print_inspect_info(info: &tpt_origin_core::lifecycle::InspectInfo) {
    println!("Name:        {}", info.name);
    println!("Type:        {}", info.service_type);
    println!("Status:      {:?}", info.status);
    if let Some(pid) = info.pid {
        println!("PID:         {pid}");
    }
    println!("Restarts:    {}", info.restart_count);
    if let Some(millis) = info.started_at_unix_millis {
        let started = std::time::UNIX_EPOCH + std::time::Duration::from_millis(millis);
        let elapsed = started.elapsed().unwrap_or_default();
        println!("Uptime:      {}", format_duration(elapsed));
    }

    if let Some(ref image) = info.image {
        println!("Image:       {image}");
    }
    if let Some(ref cmd) = info.command {
        println!("Command:     {}", cmd.join(" "));
    }
    if let Some(ref path) = info.path {
        println!("Path:        {}", path.display());
    }
    if let Some(ref wd) = info.working_dir {
        println!("Working Dir: {}", wd.display());
    }

    if !info.environment.is_empty() {
        println!("Environment:");
        for (k, v) in &info.environment {
            println!("  {k}={v}");
        }
    }

    if !info.ports.is_empty() {
        println!("Ports:");
        for p in &info.ports {
            println!("  {}:{}/{}", p.host, p.container, p.protocol);
        }
    }

    if !info.volumes.is_empty() {
        println!("Volumes:");
        for v in &info.volumes {
            let mode = if v.read_only { "ro" } else { "rw" };
            println!("  {} -> {} ({})", v.source, v.target, mode);
        }
    }

    if !info.depends_on.is_empty() {
        println!("Depends On:  {}", info.depends_on.join(", "));
    }

    if let Some(ref hc) = info.healthcheck {
        println!("Health Check:");
        println!("  Command:  {}", hc.command.join(" "));
        println!("  Interval: {}s", hc.interval_secs);
        println!("  Timeout:  {}s", hc.timeout_secs);
        println!("  Retries:  {}", hc.retries);
    }

    if let Some(ref res) = info.resources {
        println!("Resources:");
        if let Some(ref cpu) = res.cpu {
            println!("  CPU:    {cpu}");
        }
        if let Some(ref mem) = res.memory {
            println!("  Memory: {mem}");
        }
    }

    if let Some(ref sec) = info.security {
        println!("Security:");
        if !sec.cap_add.is_empty() {
            println!("  Capabilities: add {}", sec.cap_add.join(", "));
        }
        if !sec.cap_drop.is_empty() {
            println!("  Capabilities: drop {}", sec.cap_drop.join(", "));
        }
        if sec.read_only {
            println!("  Read Only:    true");
        }
        if sec.no_new_privileges {
            println!("  No New Privileges: true");
        }
    }

    if let Some(ref logging) = info.logging {
        println!("Logging:");
        println!("  Driver:   {}", logging.driver);
        if let Some(ref max_size) = logging.max_size {
            println!("  Max Size: {max_size}");
        }
        if let Some(max_file) = logging.max_file {
            println!("  Max File: {max_file}");
        }
    }

    if let Some(ref secrets) = info.secrets {
        if !secrets.is_empty() {
            println!("Secrets:");
            for s in secrets {
                println!("  {} -> {}", s.name, s.target);
            }
        }
    }
}

fn format_duration(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        let hours = secs / 3600;
        let mins = (secs % 3600) / 60;
        format!("{hours}h {mins}m")
    }
}

async fn cmd_stats(manifest_path: &PathBuf, follow: bool) -> anyhow::Result<()> {
    let content = std::fs::read_to_string(manifest_path)?;
    let manifest: tpt_origin_core::manifest::Manifest = serde_yaml::from_str(&content)?;

    let origin = tpt_origin_core::Origin::new(&manifest);

    if follow {
        // Continuous refresh mode: clear screen and reprint every 2s.
        loop {
            let stats = origin.collect_stats();
            // Clear screen and move cursor to top-left.
            print!("\x1B[2J\x1B[1;1H");
            print_stats_table(&stats);
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    } else {
        let stats = origin.collect_stats();
        print_stats_table(&stats);
    }

    Ok(())
}

fn print_stats_table(stats: &[tpt_origin_core::stats::ServiceStats]) {
    if stats.is_empty() {
        println!("No running services found. Run `tpt origin up` first.");
        return;
    }

    println!(
        "{:<20} {:>8} {:>15} {:>15} {:>12} {:>8}",
        "NAME", "CPU %", "MEM USAGE", "MEM LIMIT", "NET I/O", "UPTIME"
    );
    println!(
        "{:<20} {:>8} {:>15} {:>15} {:>12} {:>8}",
        "----", "-----", "----------", "----------", "-------", "------"
    );

    for s in stats {
        let cpu_str = s
            .cpu_percent
            .map(|c| format!("{c:.1}%"))
            .unwrap_or_else(|| "N/A".to_string());

        let mem_str = s
            .memory_rss_bytes
            .map(tpt_origin_core::stats::format_bytes)
            .unwrap_or_else(|| "N/A".to_string());

        let limit_str = s
            .memory_limit_bytes
            .map(tpt_origin_core::stats::format_bytes)
            .unwrap_or_else(|| "unlimited".to_string());

        let net_str = match (s.net_rx_bytes, s.net_tx_bytes) {
            (Some(rx), Some(tx)) => {
                format!(
                    "{}/{}",
                    tpt_origin_core::stats::format_bytes(rx),
                    tpt_origin_core::stats::format_bytes(tx)
                )
            }
            _ => "N/A".to_string(),
        };

        let uptime_str = if s.uptime_secs < 60 {
            format!("{}s", s.uptime_secs)
        } else if s.uptime_secs < 3600 {
            format!("{}m", s.uptime_secs / 60)
        } else {
            format!("{}h", s.uptime_secs / 3600)
        };

        println!(
            "{:<20} {:>8} {:>15} {:>15} {:>12} {:>8}",
            s.name, cpu_str, mem_str, limit_str, net_str, uptime_str
        );
    }
}

async fn cmd_images(filter: Option<&str>) -> anyhow::Result<()> {
    let socket = std::env::var("ORIGIN_CONTAINERD_SOCKET")
        .unwrap_or_else(|_| "/run/containerd/containerd.sock".to_string());
    let namespace = "tpt-boxcar";

    let mut args: Vec<String> = vec![
        "--address".into(),
        socket,
        "--namespace".into(),
        namespace.into(),
        "images".into(),
        "list".into(),
    ];

    if let Some(f) = filter {
        args.push(format!("name~={f}"));
    }

    let output = tokio::process::Command::new("ctr")
        .args(&args)
        .output()
        .await
        .context("failed to spawn `ctr images list` (is containerd's `ctr` CLI installed?)")?;

    if !output.status.success() {
        anyhow::bail!(
            "failed to list images: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        println!("No images found. Build or pull an image first.");
    } else {
        println!("REPOSITORY    TAG    IMAGE ID    SIZE");
        println!("------------    ---    --------    ----");
        for line in stdout.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                println!("  {trimmed}");
            }
        }
    }

    Ok(())
}

async fn cmd_rmi(image: &str) -> anyhow::Result<()> {
    let socket = std::env::var("ORIGIN_CONTAINERD_SOCKET")
        .unwrap_or_else(|_| "/run/containerd/containerd.sock".to_string());
    let namespace = "tpt-boxcar";

    // Normalize the image reference
    let image_ref = normalize_image_ref_cli(image);

    println!("Removing image: {image_ref}");

    let output = tokio::process::Command::new("ctr")
        .args([
            "--address",
            &socket,
            "--namespace",
            namespace,
            "images",
            "remove",
            &image_ref,
        ])
        .output()
        .await
        .context("failed to spawn `ctr images remove` (is containerd's `ctr` CLI installed?)")?;

    if !output.status.success() {
        anyhow::bail!(
            "failed to remove image '{image_ref}': {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    println!("Removed: {image_ref}");
    Ok(())
}

async fn cmd_pull(image: &str) -> anyhow::Result<()> {
    let socket = std::env::var("ORIGIN_CONTAINERD_SOCKET")
        .unwrap_or_else(|_| "/run/containerd/containerd.sock".to_string());
    let namespace = "tpt-boxcar";

    let image_ref = normalize_image_ref_cli(image);

    println!("Pulling: {image_ref}");

    let output = tokio::process::Command::new("ctr")
        .args([
            "--address",
            &socket,
            "--namespace",
            namespace,
            "images",
            "pull",
            &image_ref,
        ])
        .output()
        .await
        .context("failed to spawn `ctr images pull` (is containerd's `ctr` CLI installed?)")?;

    if !output.status.success() {
        anyhow::bail!(
            "failed to pull image '{image_ref}': {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    println!("Pulled: {image_ref}");
    Ok(())
}

async fn cmd_push(image: &str, config_path: Option<&PathBuf>) -> anyhow::Result<()> {
    let socket = std::env::var("ORIGIN_CONTAINERD_SOCKET")
        .unwrap_or_else(|_| "/run/containerd/containerd.sock".to_string());
    let namespace = "tpt-boxcar";

    let image_ref = normalize_image_ref_cli(image);

    // Check if the image exists locally
    let list_output = tokio::process::Command::new("ctr")
        .args([
            "--address",
            &socket,
            "--namespace",
            namespace,
            "images",
            "list",
            &format!("name=={image_ref}"),
        ])
        .output()
        .await
        .context("failed to list images")?;

    if !list_output.status.success()
        || String::from_utf8_lossy(&list_output.stdout)
            .trim()
            .is_empty()
    {
        anyhow::bail!(
            "image '{image_ref}' not found locally. Build it first with `tpt origin build`."
        );
    }

    // Build the push command
    let mut args: Vec<String> = vec![
        "--address".into(),
        socket,
        "--namespace".into(),
        namespace.into(),
        "images".into(),
        "push".into(),
    ];

    // Handle authentication via Docker config.json
    let effective_config = config_path.map(|p| p.to_path_buf()).or_else(|| {
        let home = std::env::var("HOME").ok().or({
            #[cfg(windows)]
            {
                std::env::var("USERPROFILE").ok()
            }
            #[cfg(not(windows))]
            {
                None
            }
        });
        home.map(|h| PathBuf::from(h).join(".docker/config.json"))
            .filter(|p| p.exists())
    });

    if let Some(config) = effective_config {
        if let Ok(auth_token) = extract_docker_token(&image_ref, &config) {
            args.push("--user".into());
            args.push(auth_token);
        }
    }

    args.push(image_ref.clone());

    println!("Pushing: {image_ref}");

    let output = tokio::process::Command::new("ctr")
        .args(&args)
        .output()
        .await
        .context("failed to spawn `ctr images push` (is containerd's `ctr` CLI installed?)")?;

    if !output.status.success() {
        anyhow::bail!(
            "failed to push image '{image_ref}': {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    println!("Pushed: {image_ref}");
    Ok(())
}

/// Extracts auth credentials from Docker's config.json for a given registry.
/// Returns a "user:pass" string suitable for `ctr --user`.
fn extract_docker_token(image_ref: &str, config_path: &PathBuf) -> anyhow::Result<String> {
    let config_str = std::fs::read_to_string(config_path)
        .with_context(|| format!("failed to read Docker config: {}", config_path.display()))?;
    let config: serde_json::Value = serde_json::from_str(&config_str)
        .with_context(|| format!("failed to parse Docker config: {}", config_path.display()))?;

    // Extract registry host from image reference
    let registry_host = extract_registry_host(image_ref);

    // Try auths[registry].auth (base64 encoded "user:pass")
    if let Some(auths) = config.get("auths") {
        if let Some(registry_config) = auths.get(&registry_host) {
            if let Some(auth) = registry_config.get("auth").and_then(|v| v.as_str()) {
                let decoded = base64_decode(auth)?;
                return Ok(decoded);
            }
            // Try identitytoken
            if let Some(token) = registry_config
                .get("identitytoken")
                .and_then(|v| v.as_str())
            {
                if !token.is_empty() {
                    return Ok(token.to_string());
                }
            }
        }
    }

    // Try credsStore
    if let Some(creds_store) = config.get("credsStore").and_then(|v| v.as_str()) {
        return get_credential_from_store(&registry_host, creds_store);
    }

    anyhow::bail!(
        "no credentials found for registry '{registry_host}' in {}",
        config_path.display()
    )
}

/// Extracts the registry host from an image reference.
/// e.g. "ghcr.io/org/image:tag" -> "ghcr.io"
///      "docker.io/library/alpine" -> "docker.io"
fn extract_registry_host(image_ref: &str) -> String {
    if !image_ref.contains('/') {
        return "docker.io".to_string();
    }

    let first_segment = image_ref.split('/').next().unwrap();
    if first_segment == "localhost" || first_segment.contains('.') || first_segment.contains(':') {
        first_segment.to_string()
    } else {
        "docker.io".to_string()
    }
}

/// Simple base64 decoding for Docker auth tokens.
fn base64_decode(encoded: &str) -> anyhow::Result<String> {
    use base64::Engine;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .context("failed to decode base64 auth token")?;
    String::from_utf8(decoded).context("decoded auth token is not valid UTF-8")
}

/// Queries a credential helper store for registry credentials.
fn get_credential_from_store(registry: &str, store: &str) -> anyhow::Result<String> {
    let store_cmd = match store {
        "desktop" => "docker-credential-desktop",
        "wincred" => "docker-credential-wincred",
        "osxkeychain" => "docker-credential-osxkeychain",
        _ => return Err(anyhow::anyhow!("unsupported credential store: {store}")),
    };

    let output = std::process::Command::new(store_cmd)
        .arg("get")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(mut stdin) = child.stdin.take() {
                write!(stdin, "{registry}")?;
            }
            child.wait_with_output()
        })
        .context(format!("failed to run credential helper: {store_cmd}"))?;

    if !output.status.success() {
        anyhow::bail!(
            "credential helper failed for '{registry}': {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let response: serde_json::Value = serde_json::from_slice(&output.stdout)
        .context("failed to parse credential helper response")?;

    let username = response
        .get("Username")
        .and_then(|v| v.as_str())
        .unwrap_or("<token>");
    let secret = response
        .get("Secret")
        .and_then(|v| v.as_str())
        .context("credential helper response missing Secret")?;

    Ok(format!("{username}:{secret}"))
}

/// Normalizes a bare image name to a fully qualified Docker Hub reference,
/// mirroring `containerd::normalize_image_ref` for CLI use.
fn normalize_image_ref_cli(image_ref: &str) -> String {
    if !image_ref.contains('/') {
        return format!("docker.io/library/{image_ref}");
    }

    let first_segment = image_ref.split('/').next().unwrap();
    let has_registry_host =
        first_segment == "localhost" || first_segment.contains('.') || first_segment.contains(':');

    if has_registry_host {
        image_ref.to_string()
    } else {
        format!("docker.io/{image_ref}")
    }
}

#[cfg(target_os = "linux")]
async fn cmd_build(
    dockerfile_path: &PathBuf,
    context_dir: Option<&PathBuf>,
    tag: &str,
    build_args: &[String],
) -> anyhow::Result<()> {
    use std::collections::HashMap;

    // Resolve the Dockerfile path to absolute.
    let dockerfile_path = std::fs::canonicalize(dockerfile_path).with_context(|| {
        format!(
            "failed to resolve Dockerfile path: {}",
            dockerfile_path.display()
        )
    })?;

    if !dockerfile_path.exists() {
        anyhow::bail!("Dockerfile not found: {}", dockerfile_path.display());
    }

    // Resolve context directory: defaults to the directory containing the Dockerfile.
    let context_dir = match context_dir {
        Some(ctx) => std::fs::canonicalize(ctx)
            .with_context(|| format!("failed to resolve build context: {}", ctx.display()))?,
        None => dockerfile_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_path_buf(),
    };

    if !context_dir.is_dir() {
        anyhow::bail!(
            "build context is not a directory: {}",
            context_dir.display()
        );
    }

    // Parse --build-arg KEY=VALUE pairs.
    let mut args = HashMap::new();
    for arg in build_args {
        let (key, value) = arg.split_once('=').ok_or_else(|| {
            anyhow::anyhow!("invalid --build-arg format '{arg}' (expected KEY=VALUE)")
        })?;
        args.insert(key.to_string(), value.to_string());
    }

    // Resolve the containerd socket.
    let socket_path = std::env::var("ORIGIN_CONTAINERD_SOCKET")
        .unwrap_or_else(|_| "/run/containerd/containerd.sock".to_string());
    let socket_path = std::path::PathBuf::from(socket_path);

    if !socket_path.exists() {
        anyhow::bail!(
            "containerd socket not found at {} — is containerd running? \
             try `systemctl status containerd`",
            socket_path.display()
        );
    }

    println!("Building from: {}", dockerfile_path.display());
    println!("Context:       {}", context_dir.display());
    println!("Tag:           {tag}");

    let config = tpt_origin_core::build::BuildConfig {
        context_dir,
        dockerfile_path,
        tag: tag.to_string(),
        build_args: args,
        socket_path,
    };

    let result = tpt_origin_core::build::build(&config)?;

    println!("\nBuild complete:");
    println!("  Image:    {}", result.image_ref);
    println!("  Stages:   {}", result.stages_built);
    println!("  Layers:   {}", result.layers_created);
    println!("\nRun with: tpt origin up");

    Ok(())
}

#[cfg(not(target_os = "linux"))]
async fn cmd_build(
    _dockerfile_path: &PathBuf,
    _context_dir: Option<&PathBuf>,
    _tag: &str,
    _build_args: &[String],
) -> anyhow::Result<()> {
    anyhow::bail!(
        "tpt origin build requires Linux with containerd — \
         this platform is not supported for image building"
    )
}

async fn cmd_events(
    manifest_path: &PathBuf,
    filter: Option<&str>,
    service_filter: Option<&str>,
    json_output: bool,
) -> anyhow::Result<()> {
    if !manifest_path.exists() {
        anyhow::bail!("Manifest not found: {}", manifest_path.display());
    }

    let content = std::fs::read_to_string(manifest_path)?;
    let manifest: tpt_origin_core::manifest::Manifest = serde_yaml::from_str(&content)?;

    let origin = tpt_origin_core::Origin::new(&manifest);
    let mut rx = origin.event_bus().subscribe();

    println!("Listening for lifecycle events... (Ctrl+C to stop)\n");

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                println!("\nStopped listening for events.");
                break;
            }
            Ok(event) = rx.recv() => {
                // Apply filters
                if let Some(f) = filter {
                    let event_type_str = format!("{:?}", event.event_type).to_lowercase();
                    if !event_type_str.contains(f) {
                        continue;
                    }
                }
                if let Some(s) = service_filter {
                    if event.service != s {
                        continue;
                    }
                }

                if json_output {
                    println!("{}", serde_json::to_string(&event)?);
                } else {
                    let timestamp = chrono_timestamp(event.timestamp_ms);
                    let event_type = format!("{:?}", event.event_type);
                    let detail = match &event.detail {
                        tpt_origin_core::lifecycle::EventDetail::None => String::new(),
                        tpt_origin_core::lifecycle::EventDetail::Failure(msg) => format!(" ({msg})"),
                        tpt_origin_core::lifecycle::EventDetail::HealthRetries(n) => format!(" (retry {n})"),
                        tpt_origin_core::lifecycle::EventDetail::Network(name) => format!(" ({name})"),
                        tpt_origin_core::lifecycle::EventDetail::Port(host, container) => format!(" ({host}:{container})"),
                        tpt_origin_core::lifecycle::EventDetail::StatusChange(from, to) => format!(" ({from:?} -> {to:?})"),
                    };
                    let service_name = &event.service;
                    println!("[{timestamp}] {event_type} {service_name}{detail}");
                }
            }
        }
    }

    Ok(())
}

/// Converts Unix-epoch milliseconds to a human-readable timestamp.
fn chrono_timestamp(ms: u64) -> String {
    let secs = ms / 1000;
    let millis = ms % 1000;
    let datetime = std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs);
    let elapsed = datetime
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = elapsed.as_secs();
    let hours = (total_secs / 3600) % 24;
    let minutes = (total_secs / 60) % 60;
    let seconds = total_secs % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}.{millis:03}")
}

/// Copies files to/from containers using `ctr tasks exec` with tar pipes.
/// Format: `container:path` for container paths, plain paths for host.
async fn cmd_cp(source: &str, destination: &str) -> anyhow::Result<()> {
    let (src_container, src_path) = parse_cp_path(source);
    let (dst_container, dst_path) = parse_cp_path(destination);

    match (src_container, dst_container) {
        // Host -> Container
        (None, Some(container_id)) => {
            copy_to_container(container_id, &src_path, &dst_path).await?;
        }
        // Container -> Host
        (Some(container_id), None) => {
            copy_from_container(container_id, &src_path, &dst_path).await?;
        }
        // Container -> Container
        (Some(_), Some(_)) => {
            anyhow::bail!(
                "container-to-container copy is not supported; copy via host as intermediate"
            );
        }
        // Host -> Host
        (None, None) => {
            // Standard file copy
            let src = std::path::Path::new(&src_path);
            let dst = std::path::Path::new(&dst_path);
            if src.is_dir() {
                copy_dir_recursive(src, dst)?;
            } else {
                std::fs::copy(src, dst)?;
            }
            println!("Copied {source} -> {destination}");
        }
    }

    Ok(())
}

/// Parses a cp path into (container_name, path) or (None, path) for host paths.
/// A container path starts with a container name followed by `:`.
fn parse_cp_path(path: &str) -> (Option<&str>, String) {
    if let Some(colon_pos) = path.find(':') {
        let container = &path[..colon_pos];
        let file_path = &path[colon_pos + 1..];
        // Validate container name (alphanumeric, hyphens, underscores)
        if !container.is_empty()
            && container
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            return (Some(container), file_path.to_string());
        }
    }
    (None, path.to_string())
}

/// Copies a file or directory from the host into a container.
async fn copy_to_container(container_id: &str, src: &str, dst: &str) -> anyhow::Result<()> {
    let socket = std::env::var("ORIGIN_CONTAINERD_SOCKET")
        .unwrap_or_else(|_| "/run/containerd/containerd.sock".to_string());
    let namespace = "tpt-boxcar";

    let src_path = std::path::Path::new(src);
    if !src_path.exists() {
        anyhow::bail!("source path does not exist: {src}");
    }

    // Create a tar of the source and pipe it to the container via ctr tasks exec
    let mut tar_cmd = std::process::Command::new("tar");
    tar_cmd.arg("-cf").arg("-").arg("-C");
    tar_cmd.arg(
        src_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_string_lossy()
            .to_string(),
    );
    tar_cmd.arg(
        src_path
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("."))
            .to_string_lossy()
            .to_string(),
    );

    let tar_output = tar_cmd.output().context("failed to create tar archive")?;

    if !tar_output.status.success() {
        anyhow::bail!(
            "failed to create tar archive: {}",
            String::from_utf8_lossy(&tar_output.stderr)
        );
    }

    // Create destination directory if needed
    let mkdir_status = tokio::process::Command::new("ctr")
        .args([
            "--address",
            &socket,
            "--namespace",
            namespace,
            "tasks",
            "exec",
            "--exec-id",
            &format!("cp-mkdir-{}", std::process::id()),
            container_id,
            "mkdir",
            "-p",
            dst,
        ])
        .output()
        .await
        .context("failed to create destination directory")?;

    if !mkdir_status.status.success() {
        tracing::warn!(
            "mkdir -p {dst} failed (may already exist): {}",
            String::from_utf8_lossy(&mkdir_status.stderr)
        );
    }

    // Pipe tar output to container's tar -xf
    let mut ctr_cmd = std::process::Command::new("ctr");
    ctr_cmd.args([
        "--address",
        &socket,
        "--namespace",
        namespace,
        "tasks",
        "exec",
        "--exec-id",
        &format!("cp-{}", std::process::id()),
        container_id,
        "tar",
        "-xf",
        "-",
        "-C",
        dst,
    ]);
    ctr_cmd.stdin(std::process::Stdio::piped());

    let mut child = ctr_cmd
        .spawn()
        .context("failed to spawn ctr tasks exec for cp")?;

    if let Some(mut stdin) = child.stdin.take() {
        std::io::Write::write_all(&mut stdin, &tar_output.stdout)
            .context("failed to write tar data to container")?;
    }

    let status = child.wait().context("failed to wait for cp command")?;

    if !status.success() {
        anyhow::bail!("failed to copy to container: exit status {status}");
    }

    println!("Copied {src} -> {container_id}:{dst}");
    Ok(())
}

/// Copies a file or directory from a container to the host.
async fn copy_from_container(container_id: &str, src: &str, dst: &str) -> anyhow::Result<()> {
    let socket = std::env::var("ORIGIN_CONTAINERD_SOCKET")
        .unwrap_or_else(|_| "/run/containerd/containerd.sock".to_string());
    let namespace = "tpt-boxcar";

    // Use ctr tasks exec to tar the source and capture output
    let output = tokio::process::Command::new("ctr")
        .args([
            "--address",
            &socket,
            "--namespace",
            namespace,
            "tasks",
            "exec",
            "--exec-id",
            &format!("cp-{}", std::process::id()),
            container_id,
            "tar",
            "-cf",
            "-",
            "-C",
            std::path::Path::new(src)
                .parent()
                .unwrap_or_else(|| std::path::Path::new("/"))
                .to_string_lossy()
                .to_string()
                .as_str(),
            std::path::Path::new(src)
                .file_name()
                .unwrap_or_else(|| std::ffi::OsStr::new("."))
                .to_string_lossy()
                .to_string()
                .as_str(),
        ])
        .output()
        .await
        .context("failed to exec tar in container")?;

    if !output.status.success() {
        anyhow::bail!(
            "failed to read from container: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // Extract the tar on the host
    let dst_path = std::path::Path::new(dst);
    if !dst_path.exists() {
        if dst.ends_with('/') || dst.ends_with('\\') || dst_path.extension().is_none() {
            // Destination is a directory (or looks like one)
            std::fs::create_dir_all(dst)?;
        } else {
            // Destination is a file - ensure parent exists
            if let Some(parent) = dst_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
        }
    }

    let mut tar_cmd = std::process::Command::new("tar");
    tar_cmd.arg("-xf").arg("-").arg("-C").arg(dst);

    let mut child = tar_cmd
        .stdin(std::process::Stdio::piped())
        .spawn()
        .context("failed to spawn tar for extraction")?;

    if let Some(mut stdin) = child.stdin.take() {
        std::io::Write::write_all(&mut stdin, &output.stdout)
            .context("failed to write tar data for extraction")?;
    }

    let status = child.wait().context("failed to wait for tar extraction")?;

    if !status.success() {
        anyhow::bail!("failed to extract from container: exit status {status}");
    }

    println!("Copied {container_id}:{src} -> {dst}");
    Ok(())
}

/// Recursively copies a directory.
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> anyhow::Result<()> {
    if !dst.exists() {
        std::fs::create_dir_all(dst)?;
    }
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

// Every branch below bails on some platform/service-type combination but not
// others (e.g. the OCI branch only diverges when `target_os != "linux"`), so
// the trailing `Ok(())` is unreachable on some cfg combinations and not
// others — expected, not a real dead-code bug.
#[allow(unreachable_code)]
async fn cmd_pause(service: &str) -> anyhow::Result<()> {
    let dir = std::path::Path::new(".");
    match state::read(dir)? {
        Some(env) => {
            let svc = env
                .services
                .iter()
                .find(|s| s.name == service)
                .ok_or_else(|| {
                    anyhow::anyhow!("service '{service}' not found in running environment")
                })?;

            if svc.service_type == "oci" {
                // OCI services are frozen via containerd's real cgroup freezer
                // (Tasks.Pause), not a signal — the container id is the
                // service name (see RuntimeManager::start_oci).
                #[cfg(target_os = "linux")]
                {
                    let client = tpt_origin_core::containerd::ContainerdClient::connect().await?;
                    client.pause_container(service).await?;
                    println!("Paused {service} (cgroup freezer)");
                }
                #[cfg(not(target_os = "linux"))]
                {
                    anyhow::bail!("pausing OCI services requires containerd, which is Linux-only");
                }
            } else if let Some(pid) = svc.pid {
                #[cfg(unix)]
                {
                    use libc::{kill, SIGSTOP};
                    unsafe {
                        kill(pid as i32, SIGSTOP);
                    }
                    println!("Paused {service} (PID: {pid})");
                }
                #[cfg(not(unix))]
                {
                    let _ = pid;
                    anyhow::bail!(
                        "pause is only supported on Unix/Linux systems for process services"
                    );
                }
            } else {
                anyhow::bail!("service '{service}' has no PID (may not be a process service)");
            }
        }
        None => anyhow::bail!("no running environment found (run `tpt origin up` first)"),
    }
    Ok(())
}

#[allow(unreachable_code)]
async fn cmd_unpause(service: &str) -> anyhow::Result<()> {
    let dir = std::path::Path::new(".");
    match state::read(dir)? {
        Some(env) => {
            let svc = env
                .services
                .iter()
                .find(|s| s.name == service)
                .ok_or_else(|| {
                    anyhow::anyhow!("service '{service}' not found in running environment")
                })?;

            if svc.service_type == "oci" {
                #[cfg(target_os = "linux")]
                {
                    let client = tpt_origin_core::containerd::ContainerdClient::connect().await?;
                    client.unpause_container(service).await?;
                    println!("Unpaused {service} (cgroup freezer)");
                }
                #[cfg(not(target_os = "linux"))]
                {
                    anyhow::bail!(
                        "unpausing OCI services requires containerd, which is Linux-only"
                    );
                }
            } else if let Some(pid) = svc.pid {
                #[cfg(unix)]
                {
                    use libc::{kill, SIGCONT};
                    unsafe {
                        kill(pid as i32, SIGCONT);
                    }
                    println!("Unpaused {service} (PID: {pid})");
                }
                #[cfg(not(unix))]
                {
                    let _ = pid;
                    anyhow::bail!(
                        "unpause is only supported on Unix/Linux systems for process services"
                    );
                }
            } else {
                anyhow::bail!("service '{service}' has no PID (may not be a process service)");
            }
        }
        None => anyhow::bail!("no running environment found (run `tpt origin up` first)"),
    }
    Ok(())
}
