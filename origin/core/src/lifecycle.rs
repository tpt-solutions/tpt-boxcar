use anyhow::Result;
use std::collections::HashMap;
use std::time::Instant;

use crate::dns::DnsResolver;
use crate::manifest::Manifest;
use crate::network::NetworkManager;
use crate::runtime::{RuntimeManager, ServiceStatus};

#[derive(Debug, Clone)]
pub struct ServiceHealth {
    pub name: String,
    pub status: ServiceStatus,
    pub started_at: Option<Instant>,
    pub restart_count: u32,
}

pub struct LifecycleManager {
    runtime: RuntimeManager,
    network: NetworkManager,
    dns: DnsResolver,
    health: HashMap<String, ServiceHealth>,
    manifest_name: String,
}

impl LifecycleManager {
    pub fn new(manifest: &Manifest) -> Self {
        Self {
            runtime: RuntimeManager::new(),
            network: NetworkManager::new(),
            dns: DnsResolver::new(),
            health: HashMap::new(),
            manifest_name: manifest.name.clone(),
        }
    }

    pub async fn up(&mut self, manifest: &Manifest) -> Result<()> {
        tracing::info!("Bringing up environment: {}", manifest.name);

        for (name, net_config) in &manifest.networks {
            self.network
                .create_network(crate::network::NetworkConfig {
                    name: name.clone(),
                    driver: net_config.driver.clone(),
                    subnet: None,
                    gateway: None,
                })
                .await?;
        }

        for (name, _service) in &manifest.services {
            self.dns
                .add_entry(name, &format!("10.0.0.{}", self.health.len() + 2), None);
            self.health.insert(
                name.clone(),
                ServiceHealth {
                    name: name.clone(),
                    status: ServiceStatus::Starting,
                    started_at: None,
                    restart_count: 0,
                },
            );
        }

        self.dns.start().await?;

        for (name, service) in &manifest.services {
            self.runtime.start_service(name, service).await?;
            if let Some(health) = self.health.get_mut(name) {
                health.status = ServiceStatus::Running;
                health.started_at = Some(Instant::now());
            }
        }

        tracing::info!("All services started for: {}", manifest.name);
        Ok(())
    }

    pub async fn down(&mut self) -> Result<()> {
        tracing::info!("Tearing down environment: {}", self.manifest_name);
        self.runtime.stop_all().await?;
        self.dns.stop().await?;
        tracing::info!("Environment torn down: {}", self.manifest_name);
        Ok(())
    }

    pub async fn restart_service(&mut self, name: &str, manifest: &Manifest) -> Result<()> {
        tracing::info!("Restarting service: {name}");
        self.runtime.stop_service(name).await?;
        if let Some(service) = manifest.services.get(name) {
            self.runtime.start_service(name, service).await?;
            if let Some(health) = self.health.get_mut(name) {
                health.restart_count += 1;
                health.status = ServiceStatus::Running;
                health.started_at = Some(Instant::now());
            }
        }
        Ok(())
    }

    pub fn get_service_status(&self, name: &str) -> Option<&ServiceHealth> {
        self.health.get(name)
    }

    pub fn list_services(&self) -> &HashMap<String, ServiceHealth> {
        &self.health
    }

    pub async fn wait_for_signal(&self) -> Result<()> {
        tokio::signal::ctrl_c().await?;
        tracing::info!("Received shutdown signal");
        Ok(())
    }
}
