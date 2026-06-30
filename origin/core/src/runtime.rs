use anyhow::Result;
use std::collections::HashMap;

use crate::manifest::{OCIService, Service, WasmService};

pub enum ContainerRuntime {
    OCI,
    Wasm,
}

pub struct RunningService {
    pub name: String,
    pub runtime: ContainerRuntime,
    pub status: ServiceStatus,
    pub pid: Option<u32>,
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
            },
        );
        Ok(())
    }

    pub async fn stop_service(&mut self, name: &str) -> Result<()> {
        if let Some(svc) = self.services.get_mut(name) {
            tracing::info!("Stopping service: {name}");
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
}

impl Default for RuntimeManager {
    fn default() -> Self {
        Self::new()
    }
}
