use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tpt_scope_agent::probes::{WasmInvocationEvent, WasmProbe};
use wasmtime_wasi::preview1::WasiP1Ctx;
use wasmtime_wasi::WasiCtxBuilder;

use crate::manifest::{OCIService, ProcessService, Service, WasmService};

#[cfg(all(target_os = "linux", feature = "containerd"))]
use crate::containerd::{ContainerSpec, ContainerdClient};

/// Parses a docker-style memory limit string (`"512m"`, `"1g"`, `"128Mi"`,
/// a bare byte count) into a byte count for `ContainerSpec.memory_limit_bytes`.
/// Returns `None` (rather than a default) on anything it can't parse, so
/// callers can decide whether to warn instead of silently guessing.
#[cfg(all(target_os = "linux", feature = "containerd"))]
fn parse_memory_limit(value: &str) -> Option<u64> {
    let value = value.trim();
    let (number_part, multiplier) = if let Some(n) = value.strip_suffix("Gi").or_else(|| value.strip_suffix("gi")) {
        (n, 1024 * 1024 * 1024)
    } else if let Some(n) = value.strip_suffix("Mi").or_else(|| value.strip_suffix("mi")) {
        (n, 1024 * 1024)
    } else if let Some(n) = value.strip_suffix("Ki").or_else(|| value.strip_suffix("ki")) {
        (n, 1024)
    } else if let Some(n) = value.strip_suffix('g').or_else(|| value.strip_suffix('G')) {
        (n, 1024 * 1024 * 1024)
    } else if let Some(n) = value.strip_suffix('m').or_else(|| value.strip_suffix('M')) {
        (n, 1024 * 1024)
    } else if let Some(n) = value.strip_suffix('k').or_else(|| value.strip_suffix('K')) {
        (n, 1024)
    } else if let Some(n) = value.strip_suffix('b').or_else(|| value.strip_suffix('B')) {
        (n, 1)
    } else {
        (value, 1)
    };
    number_part.trim().parse::<u64>().ok().map(|n| n * multiplier)
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
    containerd_container_id: Option<String>,
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
}

/// Store state for a Wasm service instance: just a WASI preview1 context,
/// since `WasmService` uses CLI-style args/env (unlike Frontier's plugins,
/// which speak a custom host ABI and have no WASI dependency at all).
struct OriginWasiState {
    wasi: WasiP1Ctx,
}

/// Compiles, instantiates, and invokes a Wasm module's `_start` entry point
/// with the given args/env. Deliberately takes no `RuntimeManager` state so
/// it can also be driven standalone (e.g. by an offline replay path) without
/// pulling in the rest of Origin's bookkeeping.
pub fn instantiate_and_run(
    wasm_bytes: &[u8],
    args: &[String],
    env: &HashMap<String, String>,
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

    let mut store = wasmtime::Store::new(&engine, OriginWasiState { wasi });
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
        match service {
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

        client
            .pull_image(&service.image)
            .await
            .with_context(|| format!("failed to pull image '{}' for service '{name}'", service.image))?;

        let memory_limit_bytes = service
            .resources
            .as_ref()
            .and_then(|r| r.memory.as_deref())
            .and_then(parse_memory_limit);
        let cpu_limit = service
            .resources
            .as_ref()
            .and_then(|r| r.cpu.as_deref())
            .and_then(|s| s.trim().parse::<f64>().ok());

        let spec = ContainerSpec {
            image_ref: service.image.clone(),
            command: service.command.clone(),
            env: service.environment.clone(),
            memory_limit_bytes,
            cpu_limit,
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

        let status = match instantiate_and_run(&wasm_bytes, &args, &env) {
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

        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args);
        cmd.envs(&service.environment);
        if let Some(dir) = &service.working_dir {
            cmd.current_dir(dir);
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
            environment: HashMap::new(),
            memory_limit: None,
            depends_on: vec![],
            expected_signature: None,
            trusted_public_key: None,
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

        assert!(result.is_err(), "tampered wasm bytes must fail verification");
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
        assert_eq!(
            event.env.get("TETHER_CALLER_ID"),
            Some(&"api".to_string())
        );

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
}
