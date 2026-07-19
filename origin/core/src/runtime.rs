use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tpt_scope_agent::probes::{WasmInvocationEvent, WasmProbe};
use wasmtime_wasi::preview1::WasiP1Ctx;
use wasmtime_wasi::WasiCtxBuilder;

use crate::envfile;
use crate::logging::{self, LogRotation};
use crate::manifest::{Manifest, OCIService, ProcessService, RestartPolicy, Service, WasmService};
use crate::security;

#[cfg(all(target_os = "linux", feature = "containerd"))]
use crate::containerd::{ContainerSpec, ContainerdClient};

/// Resolves an `OCIService.volumes[].source` to a real host path for a bind
/// mount. A path-shaped value (starts with `/`, `.`, or `~`) is used as-is;
/// anything else is treated as a Docker-style named volume and resolved
/// (creating the directory if needed) under a managed directory — mirroring
/// how Docker itself keeps named volumes under a fixed location rather than
/// requiring the manifest author to know a real path.
#[cfg(all(target_os = "linux", feature = "containerd"))]
fn resolve_volume_source(source: &str) -> Result<String> {
    if source.starts_with('/') || source.starts_with('.') || source.starts_with('~') {
        return Ok(source.to_string());
    }
    let base = std::env::var("ORIGIN_VOLUMES_DIR")
        .unwrap_or_else(|_| "/var/lib/tpt-boxcar/volumes".to_string());
    let path = std::path::PathBuf::from(&base).join(source);
    std::fs::create_dir_all(&path).with_context(|| {
        format!(
            "failed to create managed volume directory {}",
            path.display()
        )
    })?;
    Ok(path.to_string_lossy().to_string())
}

/// Hex-encoded sha256 of the given bytes, used both for signature
/// verification context and to tag captured invocations with the exact
/// module content that produced them.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContainerRuntime {
    OCI,
    Wasm,
    Process,
}

pub struct RunningService {
    pub name: String,
    pub runtime: ContainerRuntime,
    pub status: ServiceStatus,
    pub pid: Option<u32>,
    child: Option<tokio::process::Child>,
    /// Set only for `type: oci` services started via containerd, so
    /// `stop_service` knows which containerd container/task to tear down.
    /// Only read on Linux+`containerd` builds; other targets can't run OCI
    /// services at all (see `start_oci`'s non-Linux stub).
    #[cfg_attr(
        not(all(target_os = "linux", feature = "containerd")),
        allow(dead_code)
    )]
    containerd_container_id: Option<String>,
    /// Consecutive failed `healthcheck` probes, reset to 0 on any success.
    /// Compared against `HealthCheck.retries` to decide when to actually
    /// mark the service `Failed` rather than transiently `HealthChecking`.
    #[cfg_attr(
        not(all(target_os = "linux", feature = "containerd")),
        allow(dead_code)
    )]
    health_failures: u32,
    /// When `poll_healthchecks` last actually ran a probe for this service,
    /// so calls more frequent than `HealthCheck.interval_secs` are no-ops
    /// rather than hammering the container with exec probes.
    #[cfg_attr(
        not(all(target_os = "linux", feature = "containerd")),
        allow(dead_code)
    )]
    last_health_check: Option<std::time::Instant>,
    /// Optional log rotation handle for process/wasm services whose
    /// `logging.driver` is `"file"`. When present, the child process's
    /// stdout/stderr is redirected through this writer. Kept alive on the
    /// `RunningService` so the file handle stays open for the service's
    /// lifetime.
    #[allow(dead_code)]
    log_rotation: Option<LogRotation>,
}

/// Serializable view of a [`RunningService`], for embedders that hold an
/// [`crate::Origin`] facade rather than [`RuntimeManager`] directly. Omits
/// `child` (a live `tokio::process::Child` handle, not serializable and not
/// meaningful outside this process).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunningServiceDto {
    pub name: String,
    pub runtime: ContainerRuntime,
    pub status: ServiceStatus,
    pub pid: Option<u32>,
}

impl From<&RunningService> for RunningServiceDto {
    fn from(svc: &RunningService) -> Self {
        Self {
            name: svc.name.clone(),
            runtime: svc.runtime,
            status: svc.status.clone(),
            pid: svc.pid,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ServiceStatus {
    Starting,
    Running,
    Stopped,
    Failed(String),
    HealthChecking,
    Paused,
}

/// Store state for a Wasm service instance: just a WASI preview1 context,
/// since `WasmService` uses CLI-style args/env (unlike Frontier's plugins,
/// which speak a custom host ABI and have no WASI dependency at all).
struct OriginWasiState {
    wasi: WasiP1Ctx,
    limiter: Option<crate::reslimit::WasmResourceLimiter>,
}

/// Compiles, instantiates, and invokes a Wasm module's `_start` entry point
/// with the given args/env. Deliberately takes no `RuntimeManager` state so
/// it can also be driven standalone (e.g. by an offline replay path) without
/// pulling in the rest of Origin's bookkeeping.
///
/// `memory_limit_bytes`, when set, caps the module's linear memory growth via
/// a `wasmtime::ResourceLimiter` — denied growth traps the module rather than
/// letting it exhaust host memory.
pub fn instantiate_and_run(
    wasm_bytes: &[u8],
    args: &[String],
    env: &HashMap<String, String>,
    memory_limit_bytes: Option<u64>,
) -> Result<()> {
    let engine = wasmtime::Engine::new(&wasmtime::Config::new())
        .context("failed to create wasmtime engine")?;
    let module =
        wasmtime::Module::new(&engine, wasm_bytes).context("failed to compile wasm module")?;

    let mut wasi_builder = WasiCtxBuilder::new();
    wasi_builder.args(args).inherit_stdout().inherit_stderr();
    for (key, value) in env {
        wasi_builder.env(key, value);
    }
    let wasi = wasi_builder.build_p1();
    let limiter = memory_limit_bytes.map(crate::reslimit::WasmResourceLimiter::new);

    let mut store = wasmtime::Store::new(&engine, OriginWasiState { wasi, limiter });
    if memory_limit_bytes.is_some() {
        store.limiter(|state| {
            state
                .limiter
                .as_mut()
                .expect("limiter set when memory_limit_bytes is Some")
        });
    }
    let mut linker: wasmtime::Linker<OriginWasiState> = wasmtime::Linker::new(&engine);
    wasmtime_wasi::preview1::add_to_linker_sync(&mut linker, |s: &mut OriginWasiState| &mut s.wasi)
        .context("failed to wire WASI imports")?;

    let instance = linker
        .instantiate(&mut store, &module)
        .context("failed to instantiate wasm module")?;

    match instance.get_typed_func::<(), ()>(&mut store, "_start") {
        Ok(start) => start
            .call(&mut store, ())
            .context("wasm module trapped during execution")?,
        Err(_) => {
            tracing::warn!("wasm module has no `_start` export; treating load as success");
        }
    }

    Ok(())
}

/// Verifies an ed25519 signature (produced by Chisel's `sign_module`) over
/// a module's wasm bytes. Fails closed: any decode/verify error is returned
/// as `Err`, and callers must not fall through to instantiation on failure.
fn verify_wasm_signature(
    wasm_bytes: &[u8],
    expected_signature_hex: &str,
    trusted_public_key_hex: &str,
) -> Result<()> {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    let pubkey_bytes: [u8; 32] = hex::decode(trusted_public_key_hex)
        .context("trusted_public_key is not valid hex")?
        .try_into()
        .map_err(|_| anyhow::anyhow!("trusted_public_key must decode to 32 bytes"))?;
    let verifying_key =
        VerifyingKey::from_bytes(&pubkey_bytes).context("invalid trusted_public_key")?;

    let sig_bytes: [u8; 64] = hex::decode(expected_signature_hex)
        .context("expected_signature is not valid hex")?
        .try_into()
        .map_err(|_| anyhow::anyhow!("expected_signature must decode to 64 bytes"))?;
    let signature = Signature::from_bytes(&sig_bytes);

    verifying_key
        .verify(wasm_bytes, &signature)
        .context("wasm module signature verification failed")
}

pub struct RuntimeManager {
    services: HashMap<String, RunningService>,
    /// Optional replay-capture probe. Wired only if the caller opts in
    /// (`with_replay_capture`), so Origin doesn't hard-require a Scope
    /// agent to be running just to start Wasm services.
    scope_probe: Option<WasmProbe>,
    /// Lazily connected on first `type: oci` service start, so Origin
    /// doesn't require containerd to be running just to use Wasm/process
    /// services.
    #[cfg(all(target_os = "linux", feature = "containerd"))]
    containerd: Option<ContainerdClient>,
}

impl RuntimeManager {
    pub fn new() -> Self {
        Self {
            services: HashMap::new(),
            scope_probe: None,
            #[cfg(all(target_os = "linux", feature = "containerd"))]
            containerd: None,
        }
    }

    /// Enables invocation capture for deterministic replay: every
    /// `start_wasm` call records a `WasmInvocationEvent` before invoking
    /// the module's entry point.
    pub fn with_replay_capture(mut self) -> Self {
        self.scope_probe = Some(WasmProbe::new("origin-wasm-replay"));
        self
    }

    /// Drains any invocation events captured so far (e.g. for a caller to
    /// forward to Scope's ingest pipeline).
    pub fn take_captured_invocations(&mut self) -> Vec<WasmInvocationEvent> {
        let Some(probe) = self.scope_probe.as_mut() else {
            return Vec::new();
        };
        probe
            .drain_events()
            .into_iter()
            .filter_map(|event| match event.data {
                tpt_scope_agent::probes::EventData::WasmInvocation(inv) => Some(inv),
                _ => None,
            })
            .collect()
    }

    pub async fn start_service(&mut self, name: &str, service: &Service) -> Result<()> {
        tracing::info!("Starting service: {name}");

        // Load env_file variables into the service's environment before
        // dispatching to the specific runtime. Explicit `environment:`
        // values take precedence over env_file values.
        let mut service = service.clone();
        if let Some(env_files) = service.env_file() {
            let file_vars = envfile::load_env_files(env_files)?;
            let env = service.environment_mut();
            // env_file vars are underlay; explicit env overrides them
            for (k, v) in file_vars {
                env.entry(k).or_insert(v);
            }
        }

        match &service {
            Service::OCI(oci) => self.start_oci(name, oci).await,
            Service::Wasm(wasm) => self.start_wasm(name, wasm).await,
            Service::Process(process) => self.start_process(name, process).await,
        }
    }

    #[cfg(all(target_os = "linux", feature = "containerd"))]
    async fn start_oci(&mut self, name: &str, service: &OCIService) -> Result<()> {
        tracing::info!(
            "Starting OCI container: {} with image {}",
            name,
            service.image
        );

        if self.containerd.is_none() {
            self.containerd = Some(
                ContainerdClient::connect()
                    .await
                    .context("failed to connect to containerd")?,
            );
        }
        let client = self.containerd.as_ref().expect("just connected above");

        client.pull_image(&service.image).await.with_context(|| {
            format!(
                "failed to pull image '{}' for service '{name}'",
                service.image
            )
        })?;

        let memory_limit_bytes = service
            .resources
            .as_ref()
            .and_then(|r| r.memory.as_deref())
            .and_then(crate::reslimit::parse_memory_limit);
        let cpu_limit = service
            .resources
            .as_ref()
            .and_then(|r| r.cpu.as_deref())
            .and_then(|s| s.trim().parse::<f64>().ok());

        let mut mounts = Vec::with_capacity(service.volumes.len());
        for v in &service.volumes {
            match v.mount_type {
                crate::manifest::MountType::Tmpfs => {
                    // Tmpfs mounts don't need a host source
                    mounts.push(crate::containerd::MountSpec {
                        host_source: String::new(),
                        container_target: v.target.clone(),
                        read_only: v.read_only,
                        mount_type: crate::containerd::MountType::Tmpfs,
                        tmpfs_options: v.tmpfs_options.clone(),
                    });
                }
                crate::manifest::MountType::Volume => {
                    let host_source = resolve_volume_source(&v.source)?;
                    mounts.push(crate::containerd::MountSpec {
                        host_source,
                        container_target: v.target.clone(),
                        read_only: v.read_only,
                        mount_type: crate::containerd::MountType::Volume,
                        tmpfs_options: None,
                    });
                }
                crate::manifest::MountType::Bind => {
                    let host_source = resolve_volume_source(&v.source)?;
                    mounts.push(crate::containerd::MountSpec {
                        host_source,
                        container_target: v.target.clone(),
                        read_only: v.read_only,
                        mount_type: crate::containerd::MountType::Bind,
                        tmpfs_options: None,
                    });
                }
            }
        }

        // Mount secrets as read-only bind mounts
        if let Some(secrets) = &service.secrets {
            for secret in secrets {
                let source = envfile::resolve_secret_source(
                    secret
                        .source
                        .as_deref()
                        .unwrap_or_else(|| std::path::Path::new(".")),
                    std::path::Path::new("."),
                )?;
                mounts.push(crate::containerd::MountSpec {
                    host_source: source.to_string_lossy().to_string(),
                    container_target: secret.target.clone(),
                    read_only: true,
                    mount_type: crate::containerd::MountType::Bind,
                    tmpfs_options: None,
                });
            }
        }

        let spec = ContainerSpec {
            image_ref: service.image.clone(),
            command: service.command.clone(),
            env: service.environment.clone(),
            memory_limit_bytes,
            cpu_limit,
            mounts,
            security: service.security.clone(),
        };

        let pid = client
            .run_container(name, &spec)
            .await
            .with_context(|| format!("failed to run container for service '{name}'"))?;

        self.services.insert(
            name.to_string(),
            RunningService {
                name: name.to_string(),
                runtime: ContainerRuntime::OCI,
                status: ServiceStatus::Running,
                pid: Some(pid),
                child: None,
                containerd_container_id: Some(name.to_string()),
                health_failures: 0,
                last_health_check: None,
                log_rotation: None,
            },
        );
        Ok(())
    }

    /// containerd's gRPC API is exposed only over a Unix Domain Socket,
    /// which doesn't exist on Windows/macOS — `type: oci` services can't
    /// run natively on those platforms. Run under WSL2 or a Linux CI
    /// runner. Unlike the old stub, this fails loudly rather than silently
    /// recording a fake "Running" status for a container that never
    /// started.
    #[cfg(not(all(target_os = "linux", feature = "containerd")))]
    async fn start_oci(&mut self, name: &str, service: &OCIService) -> Result<()> {
        Err(anyhow::anyhow!(
            "OCI/containerd services require Linux with containerd installed; run under WSL2 or a Linux host. Service '{name}' (image '{}') was not started.",
            service.image
        ))
    }

    async fn start_wasm(&mut self, name: &str, service: &WasmService) -> Result<()> {
        tracing::info!(
            "Starting Wasm module: {} from {}",
            name,
            service.path.display()
        );
        if !service.path.exists() {
            return Err(anyhow::anyhow!(
                "Wasm module not found: {}",
                service.path.display()
            ));
        }
        let wasm_bytes = std::fs::read(&service.path)
            .with_context(|| format!("failed to read wasm module: {}", service.path.display()))?;

        match (&service.expected_signature, &service.trusted_public_key) {
            (Some(sig), Some(pubkey)) => {
                verify_wasm_signature(&wasm_bytes, sig, pubkey).with_context(|| {
                    format!("signature verification failed for wasm module '{name}'")
                })?;
                tracing::info!("wasm module '{name}' signature verified");
            }
            _ => {
                tracing::warn!("wasm module '{name}' loaded without signature verification");
            }
        }

        // Compile+instantiate+call runs synchronously on this task for now;
        // real containerd-style async scheduling is out of scope for this
        // minimal loading path.
        let args = service.args.clone();
        let mut env = service.environment.clone();
        // Present this service's manifest name to Tether as its caller
        // identity, so capability-scoped credentials can be resolved
        // without requiring a manifest change.
        env.insert("TETHER_CALLER_ID".to_string(), name.to_string());

        // Secrets: for wasm services, read each secret file and expose
        // its contents as an environment variable.
        if let Some(secrets) = &service.secrets {
            for secret in secrets {
                let source = envfile::resolve_secret_source(
                    secret
                        .source
                        .as_deref()
                        .unwrap_or_else(|| std::path::Path::new(".")),
                    std::path::Path::new("."),
                )?;
                match envfile::read_secret(&source) {
                    Ok(bytes) => {
                        let value = String::from_utf8_lossy(&bytes).to_string();
                        let env_key = secret.name.to_uppercase().replace('-', "_");
                        env.insert(env_key, value);
                    }
                    Err(e) => {
                        tracing::warn!(
                            "failed to read secret '{}' from {}: {e:#}",
                            secret.name,
                            source.display()
                        );
                    }
                }
            }
        }

        if let Some(probe) = self.scope_probe.as_mut() {
            let timestamp_ns = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64;
            probe.record_invocation(WasmInvocationEvent {
                module_name: name.to_string(),
                function: "_start".to_string(),
                args: args.clone(),
                env: env.clone(),
                wasm_sha256: sha256_hex(&wasm_bytes),
                timestamp_ns,
            });
        }

        // Prefer the structured `resources.memory` limit; fall back to the
        // legacy standalone `memory_limit` field for existing manifests.
        let memory_limit_bytes = service
            .resources
            .as_ref()
            .and_then(|r| r.memory.as_deref())
            .or(service.memory_limit.as_deref())
            .and_then(crate::reslimit::parse_memory_limit);

        let status = match instantiate_and_run(&wasm_bytes, &args, &env, memory_limit_bytes) {
            Ok(()) => ServiceStatus::Running,
            Err(e) => ServiceStatus::Failed(format!("wasm trap: {e}")),
        };

        self.services.insert(
            name.to_string(),
            RunningService {
                name: name.to_string(),
                runtime: ContainerRuntime::Wasm,
                status,
                pid: None,
                child: None,
                containerd_container_id: None,
                health_failures: 0,
                last_health_check: None,
                log_rotation: None,
            },
        );
        Ok(())
    }

    /// Spawns a real child process — unlike `start_oci`/`start_wasm`, this
    /// actually runs the given command rather than just recording bookkeeping
    /// state, since it doesn't depend on the containerd/Wasmtime integration
    /// that those two paths are still waiting on.
    async fn start_process(&mut self, name: &str, service: &ProcessService) -> Result<()> {
        let (program, args) = service
            .command
            .split_first()
            .ok_or_else(|| anyhow::anyhow!("service '{name}' has an empty command"))?;

        tracing::info!("Starting process: {name} ({})", service.command.join(" "));

        // Resolve the effective logging config: per-service override takes
        // precedence over the manifest-level default. If `driver` is
        // `"file"`, redirect the child's stdout/stderr through a rotating
        // log file.
        let effective_logging = logging::resolve_logging_config(
            &service.logging,
            &None, // manifest-level logging is resolved at the caller (LifecycleManager)
        );

        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args);
        cmd.envs(&service.environment);
        if let Some(dir) = &service.working_dir {
            cmd.current_dir(dir);
        }

        // Set up log rotation for file-based logging.
        let mut log_rotation = if effective_logging.driver == "file" {
            let log_path = crate::logging::log_path(name);
            match LogRotation::open(&log_path, &effective_logging) {
                Ok(rot) => Some(rot),
                Err(e) => {
                    tracing::warn!("failed to open log file for '{name}': {e:#}");
                    None
                }
            }
        } else {
            None
        };

        // If we have a log rotation handle, redirect child stdout/stderr to it.
        if let Some(ref mut rot) = log_rotation {
            let log_file = rot.get_ref().try_clone()?;
            cmd.stdout(std::process::Stdio::from(log_file));
            let log_file2 = rot.get_ref().try_clone()?;
            cmd.stderr(std::process::Stdio::from(log_file2));
        }

        // Secrets: for process services, read each secret file and expose
        // its contents as an environment variable with the secret's name
        // (uppercase). This lets process services access secrets without
        // file-based mounts (they share the host filesystem anyway).
        if let Some(secrets) = &service.secrets {
            for secret in secrets {
                let source = envfile::resolve_secret_source(
                    secret
                        .source
                        .as_deref()
                        .unwrap_or_else(|| std::path::Path::new(".")),
                    std::path::Path::new("."),
                )?;
                match envfile::read_secret(&source) {
                    Ok(bytes) => {
                        let value = String::from_utf8_lossy(&bytes).to_string();
                        let env_key = secret.name.to_uppercase().replace('-', "_");
                        cmd.env(&env_key, &value);
                        // Also mount the secret file as a readable path
                        // (process services can read the host filesystem)
                        tracing::debug!(
                            "injected secret '{}' as env var {} from {}",
                            secret.name,
                            env_key,
                            source.display()
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            "failed to read secret '{}' from {}: {e:#}",
                            secret.name,
                            source.display()
                        );
                    }
                }
            }
        }

        // `RLIMIT_CPU` is cumulative CPU-seconds consumed before a kill signal,
        // not a fractional-core throttle — there's no rlimit that expresses
        // "1.5 cores", so a `resources.cpu` limit can't be honored here (a
        // real throttle needs cgroup v2, which requires root). Only memory is
        // applied via `RLIMIT_AS`.
        #[cfg(unix)]
        {
            let security_config = service.security.clone().unwrap_or_default();
            let has_security = security_config.no_new_privileges
                || !security_config.cap_drop.is_empty()
                || !security_config.cap_add.is_empty();
            let resources = service.resources.clone();
            let name_owned = name.to_string();

            let apply_rlimits_and_security = move || {
                if let Some(resources) = &resources {
                    let (memory_bytes, cpu_cores) =
                        crate::reslimit::parse_resource_limits(resources);
                    if cpu_cores.is_some() {
                        tracing::warn!(
                            "service '{name_owned}' requests a CPU limit, but process services can only \
                             enforce memory limits (via RLIMIT_AS) without root/cgroups; ignoring cpu limit"
                        );
                    }
                    if memory_bytes.is_some() {
                        unsafe {
                            crate::reslimit::apply_rlimits(memory_bytes, None);
                        }
                    }
                }
                if has_security {
                    unsafe {
                        security::apply_security_pre_exec(&security_config);
                    }
                }
            };

            // Only install pre_exec hook if we have something to apply
            if service.resources.is_some() || has_security {
                unsafe {
                    cmd.pre_exec(move || {
                        apply_rlimits_and_security();
                        Ok(())
                    });
                }
            }
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| anyhow::anyhow!("failed to start '{name}': {e}"))?;
        let pid = child.id();

        // Fail fast if the process exits immediately (e.g. bad command).
        if let Ok(Some(status)) = child.try_wait() {
            return Err(anyhow::anyhow!(
                "process '{name}' exited immediately with status {status}"
            ));
        }

        self.services.insert(
            name.to_string(),
            RunningService {
                name: name.to_string(),
                runtime: ContainerRuntime::Process,
                status: ServiceStatus::Running,
                pid,
                child: Some(child),
                containerd_container_id: None,
                health_failures: 0,
                last_health_check: None,
                log_rotation,
            },
        );
        Ok(())
    }

    pub async fn stop_service(&mut self, name: &str) -> Result<()> {
        if let Some(svc) = self.services.get_mut(name) {
            tracing::info!("Stopping service: {name}");
            if let Some(child) = svc.child.as_mut() {
                if let Err(e) = child.kill().await {
                    tracing::warn!("failed to kill process for '{name}': {e}");
                }
            }

            #[cfg(all(target_os = "linux", feature = "containerd"))]
            if let Some(container_id) = svc.containerd_container_id.clone() {
                if let Some(client) = self.containerd.as_ref() {
                    if let Err(e) = client.stop_container(&container_id).await {
                        tracing::warn!("failed to stop containerd container '{container_id}': {e}");
                    }
                }
            }

            if let Some(svc) = self.services.get_mut(name) {
                svc.status = ServiceStatus::Stopped;
            }
        }
        Ok(())
    }

    pub async fn stop_all(&mut self) -> Result<()> {
        let names: Vec<String> = self.services.keys().cloned().collect();
        for name in names {
            self.stop_service(&name).await?;
        }
        Ok(())
    }

    /// Pauses a running service using cgroup freezer (Linux) or SIGSTOP (process).
    /// The service's CPU and memory usage will be frozen until unpaused.
    pub async fn pause_service(&mut self, name: &str) -> Result<()> {
        if let Some(svc) = self.services.get_mut(name) {
            tracing::info!("Pausing service: {name}");

            // For process services, send SIGSTOP
            #[cfg(unix)]
            if let Some(_child) = svc.child.as_mut() {
                use libc::{kill, SIGSTOP};
                if let Some(pid) = _child.id() {
                    unsafe {
                        kill(pid as i32, SIGSTOP);
                    }
                }
            }
            #[cfg(not(unix))]
            {
                // On Windows, we can use SuspendThread but it's not portable
                tracing::warn!("pause not fully supported on this platform for process services");
            }

            // For OCI containers, use containerd's pause task
            #[cfg(all(target_os = "linux", feature = "containerd"))]
            if let Some(container_id) = svc.containerd_container_id.clone() {
                if let Some(client) = self.containerd.as_ref() {
                    if let Err(e) = client.pause_container(&container_id).await {
                        tracing::warn!(
                            "failed to pause containerd container '{container_id}': {e}"
                        );
                    }
                }
            }

            svc.status = ServiceStatus::Paused;
        }
        Ok(())
    }

    /// Unpauses a paused service.
    pub async fn unpause_service(&mut self, name: &str) -> Result<()> {
        if let Some(svc) = self.services.get_mut(name) {
            tracing::info!("Unpausing service: {name}");

            // For process services, send SIGCONT
            #[cfg(unix)]
            if let Some(_child) = svc.child.as_mut() {
                use libc::{kill, SIGCONT};
                if let Some(pid) = _child.id() {
                    unsafe {
                        kill(pid as i32, SIGCONT);
                    }
                }
            }

            // For OCI containers, use containerd's unpause task
            #[cfg(all(target_os = "linux", feature = "containerd"))]
            if let Some(container_id) = svc.containerd_container_id.clone() {
                if let Some(client) = self.containerd.as_ref() {
                    if let Err(e) = client.unpause_container(&container_id).await {
                        tracing::warn!(
                            "failed to unpause containerd container '{container_id}': {e}"
                        );
                    }
                }
            }

            svc.status = ServiceStatus::Running;
        }
        Ok(())
    }

    /// Checks each running service for an exit its manifest didn't ask for
    /// and, if `restart_policy` allows it, actually restarts it — real
    /// crash recovery, unlike before where a crashed service just silently
    /// disappeared from tracking with no way back. Poll-based (call this
    /// periodically) rather than event-driven, since wiring a background
    /// watcher per service would need `RuntimeManager` to be shared behind
    /// `Arc<Mutex<_>>` across tasks — a larger refactor deferred for now.
    /// Returns the names of services that were restarted.
    pub async fn reap_and_restart(&mut self, manifest: &Manifest) -> Result<Vec<String>> {
        let mut restarted = Vec::new();
        let names: Vec<String> = self.services.keys().cloned().collect();

        for name in names {
            let exit = self.check_exited(&name).await;
            let Some(succeeded) = exit else { continue };

            let Some(service) = manifest.services.get(&name) else {
                continue;
            };
            let policy = service.restart_policy();
            let should_restart = match policy {
                RestartPolicy::Never => false,
                RestartPolicy::Always => true,
                RestartPolicy::OnFailure => !succeeded,
            };

            if let Some(svc) = self.services.get_mut(&name) {
                svc.status = ServiceStatus::Failed(format!(
                    "exited {}",
                    if succeeded {
                        "successfully"
                    } else {
                        "with failure"
                    }
                ));
            }

            if should_restart {
                tracing::info!(
                    "service '{name}' exited unexpectedly, restarting (policy: {policy:?})"
                );
                self.services.remove(&name);
                self.start_service(&name, service).await?;
                restarted.push(name);
            }
        }

        Ok(restarted)
    }

    /// Returns `Some(true)` if the service exited successfully, `Some(false)`
    /// if it exited with failure, or `None` if it's still running (or its
    /// liveness can't be determined, e.g. no containerd client connected).
    async fn check_exited(&mut self, name: &str) -> Option<bool> {
        // containerd task liveness must be checked before taking a mutable
        // borrow of `self.services` below, since both live on `self`.
        #[cfg(all(target_os = "linux", feature = "containerd"))]
        let containerd_container_id = self
            .services
            .get(name)
            .and_then(|s| s.containerd_container_id.clone());
        #[cfg(all(target_os = "linux", feature = "containerd"))]
        let containerd_exited = if let Some(container_id) = containerd_container_id {
            match self.containerd.as_ref() {
                Some(client) => match client.task_pid(&container_id).await {
                    Ok(None) => Some(true), // gone; treat as a clean exit (real exit code isn't surfaced by task_pid)
                    Ok(Some(_)) => None,    // still running
                    Err(_) => None, // can't determine right now; don't false-positive a restart
                },
                None => None,
            }
        } else {
            None
        };

        let svc = self.services.get_mut(name)?;
        if let Some(child) = svc.child.as_mut() {
            if let Ok(Some(status)) = child.try_wait() {
                return Some(status.success());
            }
            return None;
        }

        #[cfg(all(target_os = "linux", feature = "containerd"))]
        return containerd_exited;
        #[cfg(not(all(target_os = "linux", feature = "containerd")))]
        None
    }

    /// Polls healthchecks for every running service whose `interval_secs`
    /// has elapsed since its last probe. For OCI services, execs the
    /// configured command inside the container. For process services,
    /// runs HTTP/TCP/exec probes against the service. Drives
    /// `RunningService.status` through `HealthChecking` to `Failed` after
    /// `retries` consecutive failures.
    pub async fn poll_healthchecks(&mut self, manifest: &Manifest) -> Result<Vec<String>> {
        let mut newly_failed = Vec::new();
        let names: Vec<String> = self.services.keys().cloned().collect();

        for name in names {
            let service = match manifest.services.get(&name) {
                Some(s) => s,
                None => continue,
            };
            let healthcheck = match service {
                Service::OCI(oci) => oci.healthcheck.as_ref(),
                Service::Wasm(wasm) => wasm.healthcheck.as_ref(),
                Service::Process(process) => process.healthcheck.as_ref(),
            };
            let Some(healthcheck) = healthcheck else {
                continue;
            };

            let due = self.services.get(&name).is_some_and(|svc| {
                svc.last_health_check
                    .map(|t| {
                        t.elapsed() >= std::time::Duration::from_secs(healthcheck.interval_secs)
                    })
                    .unwrap_or(true)
            });
            if !due {
                continue;
            }

            let healthy = match service {
                Service::OCI(oci) => self.run_oci_healthcheck(&name, oci, healthcheck).await,
                Service::Process(process) => {
                    self.run_process_healthcheck(&name, process, healthcheck)
                        .await
                }
                Service::Wasm(wasm) => self.run_wasm_healthcheck(&name, wasm, healthcheck).await,
            };

            if let Some(svc) = self.services.get_mut(&name) {
                svc.last_health_check = Some(std::time::Instant::now());
                if healthy {
                    svc.health_failures = 0;
                    if matches!(svc.status, ServiceStatus::HealthChecking) {
                        svc.status = ServiceStatus::Running;
                    }
                } else {
                    svc.health_failures += 1;
                    if svc.health_failures >= healthcheck.retries {
                        svc.status = ServiceStatus::Failed(format!(
                            "healthcheck failed {} consecutive times",
                            svc.health_failures
                        ));
                        newly_failed.push(name.clone());
                    } else {
                        svc.status = ServiceStatus::HealthChecking;
                    }
                }
            }
        }

        Ok(newly_failed)
    }

    #[cfg(all(target_os = "linux", feature = "containerd"))]
    async fn run_oci_healthcheck(
        &self,
        name: &str,
        _oci: &OCIService,
        healthcheck: &crate::manifest::HealthCheck,
    ) -> bool {
        use crate::manifest::CheckType;

        match healthcheck.check_type {
            CheckType::Exec => {
                let Some(container_id) = self
                    .services
                    .get(name)
                    .and_then(|s| s.containerd_container_id.clone())
                else {
                    return false;
                };
                let Some(client) = self.containerd.as_ref() else {
                    return false;
                };
                client
                    .exec_healthcheck(
                        &container_id,
                        &healthcheck.command,
                        std::time::Duration::from_secs(healthcheck.timeout_secs),
                    )
                    .await
                    .unwrap_or(false)
            }
            CheckType::Http => self.run_http_healthcheck(healthcheck).await,
            CheckType::Tcp => self.run_tcp_healthcheck(healthcheck).await,
        }
    }

    #[cfg(not(all(target_os = "linux", feature = "containerd")))]
    async fn run_oci_healthcheck(
        &self,
        _name: &str,
        _oci: &OCIService,
        _healthcheck: &crate::manifest::HealthCheck,
    ) -> bool {
        false
    }

    async fn run_process_healthcheck(
        &self,
        _name: &str,
        _process: &ProcessService,
        healthcheck: &crate::manifest::HealthCheck,
    ) -> bool {
        use crate::manifest::CheckType;

        match healthcheck.check_type {
            CheckType::Exec => {
                let Some(program) = healthcheck.command.first() else {
                    return false;
                };
                let args = &healthcheck.command[1..];
                let result = tokio::process::Command::new(program)
                    .args(args)
                    .output()
                    .await;
                match result {
                    Ok(output) => output.status.success(),
                    Err(_) => false,
                }
            }
            CheckType::Http => self.run_http_healthcheck(healthcheck).await,
            CheckType::Tcp => self.run_tcp_healthcheck(healthcheck).await,
        }
    }

    async fn run_wasm_healthcheck(
        &self,
        _name: &str,
        _wasm: &WasmService,
        healthcheck: &crate::manifest::HealthCheck,
    ) -> bool {
        use crate::manifest::CheckType;

        match healthcheck.check_type {
            CheckType::Exec => {
                let Some(program) = healthcheck.command.first() else {
                    return false;
                };
                let args = &healthcheck.command[1..];
                let result = tokio::process::Command::new(program)
                    .args(args)
                    .output()
                    .await;
                match result {
                    Ok(output) => output.status.success(),
                    Err(_) => false,
                }
            }
            CheckType::Http => self.run_http_healthcheck(healthcheck).await,
            CheckType::Tcp => self.run_tcp_healthcheck(healthcheck).await,
        }
    }

    async fn run_http_healthcheck(&self, healthcheck: &crate::manifest::HealthCheck) -> bool {
        let port = match healthcheck.port {
            Some(p) => p,
            None => return false,
        };
        let path = healthcheck.path.as_deref().unwrap_or("/");
        let url = format!("http://127.0.0.1:{port}{path}");
        let timeout = std::time::Duration::from_secs(healthcheck.timeout_secs);

        // ureq is synchronous, so spawn_blocking to avoid blocking the async runtime
        tokio::task::spawn_blocking(move || match ureq::get(&url).timeout(timeout).call() {
            Ok(response) => {
                let status = response.status();
                (200..400).contains(&status)
            }
            Err(_) => false,
        })
        .await
        .unwrap_or_default()
    }

    async fn run_tcp_healthcheck(&self, healthcheck: &crate::manifest::HealthCheck) -> bool {
        let port = match healthcheck.port {
            Some(p) => p,
            None => return false,
        };
        let timeout = std::time::Duration::from_secs(healthcheck.timeout_secs);
        let addr = format!("127.0.0.1:{port}");

        matches!(
            tokio::time::timeout(timeout, tokio::net::TcpStream::connect(&addr)).await,
            Ok(Ok(_))
        )
    }

    pub fn get_status(&self, name: &str) -> Option<&ServiceStatus> {
        self.services.get(name).map(|s| &s.status)
    }

    pub fn list_services(&self) -> &HashMap<String, RunningService> {
        &self.services
    }

    /// PIDs of services that are real OS processes (currently only
    /// `type: process` services spawn one) — used so a separate CLI
    /// invocation can kill them directly even if the parent `up` process is
    /// itself force-killed, since Windows has no process-group cascade.
    pub fn list_pids(&self) -> HashMap<String, u32> {
        self.services
            .iter()
            .filter_map(|(name, svc)| svc.pid.map(|pid| (name.clone(), pid)))
            .collect()
    }
}

impl Default for RuntimeManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::WasmService;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;
    use std::io::Write;

    fn wasm_service(path: std::path::PathBuf) -> WasmService {
        WasmService {
            path,
            args: vec![],
            ports: vec![],
            environment: HashMap::new(),
            memory_limit: None,
            resources: None,
            depends_on: vec![],
            expected_signature: None,
            trusted_public_key: None,
            restart_policy: Default::default(),
            env_file: None,
            secrets: None,
            security: None,
            logging: None,
            healthcheck: None,
            profiles: vec![],
            configs: None,
            extends: None,
        }
    }

    /// A minimal WASI-CLI-shaped module: exports `_start` and does nothing.
    fn write_hello_wasm_fixture() -> tempfile::NamedTempFile {
        let wat = r#"
            (module
                (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
                (memory (export "memory") 1)
                (func $_start (export "_start"))
            )
        "#;
        let bytes = wat::parse_str(wat).expect("valid wat fixture");
        let mut file = tempfile::Builder::new()
            .suffix(".wasm")
            .tempfile()
            .expect("create temp file");
        file.write_all(&bytes).expect("write wasm bytes");
        file
    }

    #[tokio::test]
    async fn start_wasm_runs_real_module_via_wasmtime() {
        let fixture = write_hello_wasm_fixture();
        let service = wasm_service(fixture.path().to_path_buf());

        let mut manager = RuntimeManager::new();
        manager
            .start_service("hello", &Service::Wasm(service))
            .await
            .expect("start_service should succeed");

        assert_eq!(manager.get_status("hello"), Some(&ServiceStatus::Running));
    }

    #[tokio::test]
    async fn start_wasm_missing_file_fails() {
        let service = wasm_service("/nonexistent/module.wasm".into());

        let mut manager = RuntimeManager::new();
        let result = manager
            .start_service("missing", &Service::Wasm(service))
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn start_wasm_with_matching_signature_succeeds() {
        let fixture = write_hello_wasm_fixture();
        let wasm_bytes = std::fs::read(fixture.path()).unwrap();
        let signing_key = SigningKey::generate(&mut OsRng);
        let signature = signing_key.sign(&wasm_bytes);

        let mut service = wasm_service(fixture.path().to_path_buf());
        service.expected_signature = Some(hex::encode(signature.to_bytes()));
        service.trusted_public_key = Some(hex::encode(signing_key.verifying_key().to_bytes()));

        let mut manager = RuntimeManager::new();
        manager
            .start_service("signed", &Service::Wasm(service))
            .await
            .expect("matching signature should verify and start");

        assert_eq!(manager.get_status("signed"), Some(&ServiceStatus::Running));
    }

    #[tokio::test]
    async fn start_wasm_with_tampered_file_fails_closed() {
        let fixture = write_hello_wasm_fixture();
        let original_bytes = std::fs::read(fixture.path()).unwrap();
        let signing_key = SigningKey::generate(&mut OsRng);
        let signature = signing_key.sign(&original_bytes);

        // Tamper with the file after signing.
        let mut tampered = original_bytes.clone();
        tampered.push(0x00);
        std::fs::write(fixture.path(), &tampered).unwrap();

        let mut service = wasm_service(fixture.path().to_path_buf());
        service.expected_signature = Some(hex::encode(signature.to_bytes()));
        service.trusted_public_key = Some(hex::encode(signing_key.verifying_key().to_bytes()));

        let mut manager = RuntimeManager::new();
        let result = manager
            .start_service("tampered", &Service::Wasm(service))
            .await;

        assert!(
            result.is_err(),
            "tampered wasm bytes must fail verification"
        );
    }

    #[tokio::test]
    async fn start_wasm_with_replay_capture_records_invocation() {
        let fixture = write_hello_wasm_fixture();
        let wasm_bytes = std::fs::read(fixture.path()).unwrap();
        let mut service = wasm_service(fixture.path().to_path_buf());
        service.args = vec!["--port".to_string(), "8080".to_string()];

        let mut manager = RuntimeManager::new().with_replay_capture();
        manager
            .start_service("api", &Service::Wasm(service))
            .await
            .expect("start_service should succeed");

        let captured = manager.take_captured_invocations();
        assert_eq!(captured.len(), 1);
        let event = &captured[0];
        assert_eq!(event.module_name, "api");
        assert_eq!(event.function, "_start");
        assert_eq!(event.args, vec!["--port".to_string(), "8080".to_string()]);
        assert_eq!(event.wasm_sha256, sha256_hex(&wasm_bytes));
        assert_eq!(event.env.get("TETHER_CALLER_ID"), Some(&"api".to_string()));

        // Draining clears the buffer.
        assert!(manager.take_captured_invocations().is_empty());
    }

    #[tokio::test]
    async fn no_replay_capture_by_default() {
        let fixture = write_hello_wasm_fixture();
        let service = wasm_service(fixture.path().to_path_buf());

        let mut manager = RuntimeManager::new();
        manager
            .start_service("api", &Service::Wasm(service))
            .await
            .expect("start_service should succeed");

        assert!(manager.take_captured_invocations().is_empty());
    }

    /// Not part of the normal test run — regenerates the checked-in
    /// `tests/fixtures/hello.wasm` used by the Phase 10 cold-start and
    /// multi-arch benchmarks/docs from the same WAT source as the fixture
    /// above, so all three stay in sync. Run manually with `cargo test -p
    /// tpt-origin-core -- --ignored regenerate_hello_wasm_fixture` after
    /// changing `write_hello_wasm_fixture`'s WAT.
    #[test]
    #[ignore]
    fn regenerate_hello_wasm_fixture() {
        let wasm_bytes = wat::parse_str(
            r#"
            (module
                (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
                (memory (export "memory") 1)
                (func $_start (export "_start"))
            )
        "#,
        )
        .expect("valid wat fixture");

        let out_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        std::fs::create_dir_all(&out_dir).unwrap();
        std::fs::write(out_dir.join("hello.wasm"), &wasm_bytes).unwrap();
    }

    fn short_lived_command() -> Vec<String> {
        // Must not exit *immediately* (start_process fails fast on that,
        // treating it as a bad command) but must exit well before the
        // test's own wait below, so `reap_and_restart` has something real
        // to detect.
        if cfg!(windows) {
            vec![
                "ping".to_string(),
                "-n".to_string(),
                "2".to_string(),
                "127.0.0.1".to_string(),
            ]
        } else {
            vec!["sh".to_string(), "-c".to_string(), "sleep 0.3".to_string()]
        }
    }

    #[tokio::test]
    async fn reap_and_restart_brings_back_a_crashed_process_with_always_policy() {
        let mut manager = RuntimeManager::new();
        let service = Service::Process(ProcessService {
            command: short_lived_command(),
            ports: vec![],
            environment: HashMap::new(),
            working_dir: None,
            depends_on: vec![],
            resources: None,
            restart_policy: crate::manifest::RestartPolicy::Always,
            env_file: None,
            secrets: None,
            security: None,
            logging: None,
            healthcheck: None,
            profiles: vec![],
            configs: None,
            extends: None,
        });
        let mut services = HashMap::new();
        services.insert("flaky".to_string(), service.clone());
        let manifest = Manifest {
            name: "reap-test".to_string(),
            version: String::new(),
            services,
            networks: HashMap::new(),
            volumes: HashMap::new(),
            logging: None,
            configs: HashMap::new(),
            secrets: HashMap::new(),
        };

        manager
            .start_service("flaky", &service)
            .await
            .expect("should start");
        let first_pid = manager.list_services().get("flaky").unwrap().pid;
        assert_eq!(manager.get_status("flaky"), Some(&ServiceStatus::Running));

        // Let the short-lived command actually exit.
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

        let restarted = manager
            .reap_and_restart(&manifest)
            .await
            .expect("reap should not error");
        assert_eq!(restarted, vec!["flaky".to_string()]);
        assert_eq!(
            manager.get_status("flaky"),
            Some(&ServiceStatus::Running),
            "service should be running again after restart"
        );

        let second_pid = manager.list_services().get("flaky").unwrap().pid;
        assert!(
            second_pid.is_some(),
            "restarted process should have a real pid"
        );
        assert_ne!(
            first_pid, second_pid,
            "restart should spawn a genuinely new process"
        );

        manager.stop_service("flaky").await.ok();
    }

    #[tokio::test]
    async fn reap_and_restart_leaves_a_never_policy_service_stopped() {
        let mut manager = RuntimeManager::new();
        let service = Service::Process(ProcessService {
            command: short_lived_command(),
            ports: vec![],
            environment: HashMap::new(),
            working_dir: None,
            depends_on: vec![],
            resources: None,
            restart_policy: crate::manifest::RestartPolicy::Never,
            env_file: None,
            secrets: None,
            security: None,
            logging: None,
            healthcheck: None,
            profiles: vec![],
            configs: None,
            extends: None,
        });
        let mut services = HashMap::new();
        services.insert("one-shot".to_string(), service.clone());
        let manifest = Manifest {
            name: "reap-test".to_string(),
            version: String::new(),
            services,
            networks: HashMap::new(),
            volumes: HashMap::new(),
            logging: None,
            configs: HashMap::new(),
            secrets: HashMap::new(),
        };

        manager
            .start_service("one-shot", &service)
            .await
            .expect("should start");
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

        let restarted = manager
            .reap_and_restart(&manifest)
            .await
            .expect("reap should not error");
        assert!(
            restarted.is_empty(),
            "restart_policy: never must not be restarted"
        );
        assert!(
            matches!(
                manager.get_status("one-shot"),
                Some(ServiceStatus::Failed(_))
            ),
            "service should be marked Failed, not silently forgotten"
        );
    }
}
