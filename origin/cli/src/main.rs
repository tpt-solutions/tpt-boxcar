use anyhow::Context;
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
        /// process binaries, Dockerfiles) and triggers a restart on change.
        #[arg(long)]
        watch: bool,
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
            OriginCommands::Up { manifest, watch } => cmd_up(&manifest, watch).await,
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
        },
        Commands::Up { manifest } => cmd_up(&manifest, false).await,
        Commands::Down => cmd_down().await,
    }
}

const MANIFEST_TEMPLATE: &str = include_str!("../../examples/getting-started/manifest.yaml");

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

    let mut origin = tpt_origin_core::Origin::new(&manifest);
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
    // The embeddable `Origin` facade deliberately doesn't expose
    // `wait_for_signal` (a CLI-only concern), so the CLI waits on ctrl_c
    // itself. In between, it's also the one place that actually drives the
    // poll-based `reap_and_restart`/`poll_healthchecks` loop — until now
    // both were only callable, never invoked automatically by anything.
    let mut supervise_tick = tokio::time::interval(std::time::Duration::from_secs(5));
    supervise_tick.tick().await; // first tick fires immediately; skip it
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("Received shutdown signal");
                break;
            }
            _ = supervise_tick.tick() => {
                if let Err(e) = origin.reap_and_restart(&manifest).await {
                    tracing::warn!("reap_and_restart failed: {e}");
                }
                if let Err(e) = origin.poll_healthchecks(&manifest).await {
                    tracing::warn!("poll_healthchecks failed: {e}");
                }
            }
        }
    }
    origin.down().await?;
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

    if svc.service_type != "oci" {
        anyhow::bail!(
            "real log capture is only wired up for `type: oci` services so far; \
             '{service}' is `type: {}` — its output is inherited directly into this terminal's `tpt origin up`",
            svc.service_type
        );
    }

    let path = oci_log_path(service);
    if !path.exists() {
        anyhow::bail!("no captured log file found at {} yet", path.display());
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
                        master_writer.write_all(&[b'\r'])?;
                    } else if key.code == KeyCode::Backspace {
                        master_writer.write_all(&[0x7f])?;
                    } else if let KeyCode::Esc = key.code {
                        master_writer.write_all(&[0x1b])?;
                    } else if let KeyCode::Tab = key.code {
                        master_writer.write_all(&[b'\t'])?;
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
 