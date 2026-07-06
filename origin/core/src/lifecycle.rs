use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::dns::DnsResolver;
use crate::manifest::Manifest;
use crate::network::NetworkManager;
use crate::runtime::{RuntimeManager, ServiceStatus};

/// Orders `manifest.services` into dependency-respecting "waves": each wave
/// is a list of service names whose `depends_on` are all satisfied by
/// earlier waves, so services within a wave can start concurrently and
/// waves themselves run in sequence. Errors on an unknown dependency name
/// or a circular dependency (previously: `up()` just iterated the
/// `HashMap` in arbitrary order and ignored `depends_on` entirely).
pub fn topological_waves(manifest: &Manifest) -> Result<Vec<Vec<String>>> {
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();

    for name in manifest.services.keys() {
        in_degree.entry(name.as_str()).or_insert(0);
    }

    for (name, service) in &manifest.services {
        for dep in service.depends_on() {
            anyhow::ensure!(
                manifest.services.contains_key(dep),
                "service '{}' depends_on unknown service '{}'",
                name,
                dep
            );
            *in_degree.get_mut(name.as_str()).unwrap() += 1;
            dependents.entry(dep.as_str()).or_default().push(name.as_str());
        }
    }

    let mut waves = Vec::new();
    let mut remaining = in_degree.clone();
    let mut resolved: HashSet<&str> = HashSet::new();

    while resolved.len() < in_degree.len() {
        let wave: Vec<&str> = remaining
            .iter()
            .filter(|(name, degree)| **degree == 0 && !resolved.contains(*name))
            .map(|(name, _)| *name)
            .collect();

        anyhow::ensure!(
            !wave.is_empty(),
            "circular dependency detected among services: {:?}",
            remaining.keys().filter(|n| !resolved.contains(*n)).collect::<Vec<_>>()
        );

        for name in &wave {
            resolved.insert(name);
            if let Some(deps) = dependents.get(name) {
                for dependent in deps {
                    if let Some(degree) = remaining.get_mut(dependent) {
                        *degree = degree.saturating_sub(1);
                    }
                }
            }
        }

        waves.push(wave.iter().map(|s| s.to_string()).collect());
    }

    Ok(waves)
}

#[derive(Debug, Clone)]
pub struct ServiceHealth {
    pub name: String,
    pub status: ServiceStatus,
    pub started_at: Option<Instant>,
    pub restart_count: u32,
}

/// Serializable view of a [`ServiceHealth`]. `started_at` (an `Instant`,
/// process-local and not serializable) is converted to Unix-epoch
/// milliseconds relative to when this DTO is built, since embedders care
/// about wall-clock start time, not the monotonic instant itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceHealthDto {
    pub name: String,
    pub status: ServiceStatus,
    pub started_at_unix_millis: Option<u64>,
    pub restart_count: u32,
}

impl From<&ServiceHealth> for ServiceHealthDto {
    fn from(health: &ServiceHealth) -> Self {
        let started_at_unix_millis = health.started_at.map(|instant| {
            let elapsed = instant.elapsed();
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default();
            now.saturating_sub(elapsed).as_millis() as u64
        });
        Self {
            name: health.name.clone(),
            status: health.status.clone(),
            started_at_unix_millis,
            restart_count: health.restart_count,
        }
    }
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

        let waves = topological_waves(manifest)?;
        for wave in &waves {
            tracing::debug!("starting service wave: {:?}", wave);
            for name in wave {
                let service = manifest
                    .services
                    .get(name)
                    .expect("topological_waves only returns known service names");
                self.runtime.start_service(name, service).await?;
                if let Some(health) = self.health.get_mut(name) {
                    health.status = ServiceStatus::Running;
                    health.started_at = Some(Instant::now());
                }
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

    /// PIDs of services that are real OS processes, keyed by service name.
    pub fn service_pids(&self) -> HashMap<String, u32> {
        self.runtime.list_pids()
    }

    pub async fn wait_for_signal(&self) -> Result<()> {
        tokio::signal::ctrl_c().await?;
        tracing::info!("Received shutdown signal");
        Ok(())
    }
}

#[cfg(test)]
mod topological_waves_tests {
    use super::*;
    use crate::manifest::{Manifest, ProcessService, Service};

    fn process_service(depends_on: &[&str]) -> Service {
        Service::Process(ProcessService {
            command: vec!["true".to_string()],
            environment: HashMap::new(),
            working_dir: None,
            depends_on: depends_on.iter().map(|s| s.to_string()).collect(),
        })
    }

    fn manifest_with(services: &[(&str, &[&str])]) -> Manifest {
        Manifest {
            name: "test".to_string(),
            version: String::new(),
            services: services
                .iter()
                .map(|(name, deps)| (name.to_string(), process_service(deps)))
                .collect(),
            networks: HashMap::new(),
            volumes: HashMap::new(),
        }
    }

    #[test]
    fn linear_chain_orders_correctly() {
        // c depends on b, b depends on a -> waves must be [a], [b], [c]
        let manifest = manifest_with(&[("a", &[]), ("b", &["a"]), ("c", &["b"])]);
        let waves = topological_waves(&manifest).unwrap();
        assert_eq!(waves, vec![vec!["a".to_string()], vec!["b".to_string()], vec!["c".to_string()]]);
    }

    #[test]
    fn independent_services_land_in_the_same_wave() {
        let manifest = manifest_with(&[("a", &[]), ("b", &[]), ("c", &["a", "b"])]);
        let waves = topological_waves(&manifest).unwrap();
        assert_eq!(waves.len(), 2);
        let mut first_wave = waves[0].clone();
        first_wave.sort();
        assert_eq!(first_wave, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(waves[1], vec!["c".to_string()]);
    }

    #[test]
    fn circular_dependency_is_rejected() {
        let manifest = manifest_with(&[("a", &["b"]), ("b", &["a"])]);
        let err = topological_waves(&manifest).unwrap_err();
        assert!(err.to_string().contains("circular dependency"));
    }

    #[test]
    fn unknown_dependency_is_rejected() {
        let manifest = manifest_with(&[("a", &["nonexistent"])]);
        let err = topological_waves(&manifest).unwrap_err();
        assert!(err.to_string().contains("unknown service"));
    }
}
