use anyhow::Result;
use std::collections::HashMap;

use crate::manifest::{OCIService, ProcessService, Service, WasmService};

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
}

#[derive(Debug, Clone, PartialEq)]
pub enum ServiceStatus {
    Starting,
    Running,
    Stopped,
    Failed(String),
    HealthChecking,
}

pub struct RuntimeManager {
    services: HashMap<String, RunningService>,
}

impl RuntimeManager {
    pub fn new() -> Self {
        Self {
            services: HashMap::new(),
        }
    }

    pub async fn start_service(&mut self, name: &str, service: &Service) -> Result<()> {
        tracing::info!("Starting service: {name}");
        match service {
            Service::OCI(oci) => self.start_oci(name, oci).await,
            Service::Wasm(wasm) => self.start_wasm(name, wasm).await,
            Service::Process(process) => self.start_process(name, process).await,
        }
    }

    async fn start_oci(&mut self, name: &str, service: &OCIService) -> Result<()> {
        tracing::info!(
            "Starting OCI container: {} with image {}",
            name,
            service.image
        );
        self.services.insert(
            name.to_string(),
            RunningService {
                name: name.to_string(),
                runtime: ContainerRuntime::OCI,
                status: ServiceStatus::Running,
                pid: None,
                child: None,
            },
        );
        Ok(())
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
        self.services.insert(
            name.to_string(),
            RunningService {
                name: name.to_string(),
                runtime: ContainerRuntime::Wasm,
                status: ServiceStatus::Running,
                pid: None,
                child: None,
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
            svc.status = ServiceStatus::Stopped;
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
