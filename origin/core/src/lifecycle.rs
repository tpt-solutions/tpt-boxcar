use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;

use crate::dns::DnsResolver;
use crate::manifest::{Manifest, Service};
use crate::network::NetworkManager;
use crate::portmap::PortMapper;
use crate::runtime::{RuntimeManager, ServiceStatus};

/// Maximum number of events to buffer in the broadcast channel.
/// If a subscriber is slow and the buffer fills, events are dropped.
const EVENT_CHANNEL_CAPACITY: usize = 1024;

/// Lifecycle events emitted during service transitions. Real-time event
/// stream equivalent to `docker events`, allowing external observers to
/// react to state changes without polling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleEvent {
    /// Timestamp in Unix-epoch milliseconds.
    pub timestamp_ms: u64,
    /// Type of event.
    pub event_type: EventType,
    /// Name of the affected service.
    pub service: String,
    /// Additional context for the event.
    pub detail: EventDetail,
}

/// Types of lifecycle events.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EventType {
    /// Service is starting.
    Start,
    /// Service has started successfully.
    Started,
    /// Service is being stopped.
    Stop,
    /// Service has stopped.
    Stopped,
    /// Service failed (crash, healthcheck, etc.).
    Failed,
    /// Service is being restarted.
    Restart,
    /// Health check passed.
    HealthOk,
    /// Health check failed (transient).
    HealthFail,
    /// Network attached to service.
    NetworkAttach,
    /// Network detached from service.
    NetworkDetach,
    /// Port mapping applied.
    PortMap,
    /// Environment merged (env_file loaded).
    EnvMerge,
    /// Service is paused (cgroup freezer).
    Pause,
    /// Service is unpaused.
    Unpause,
}

/// Additional detail for lifecycle events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventDetail {
    /// No additional detail.
    None,
    /// Failure message.
    Failure(String),
    /// Health check retry count.
    HealthRetries(u32),
    /// Network name.
    Network(String),
    /// Port mapping (host:container).
    Port(u16, u16),
    /// Status transition (from, to).
    StatusChange(ServiceStatus, ServiceStatus),
}

impl LifecycleEvent {
    /// Creates a new lifecycle event with the current timestamp.
    fn new(event_type: EventType, service: impl Into<String>, detail: EventDetail) -> Self {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        Self {
            timestamp_ms,
            event_type,
            service: service.into(),
            detail,
        }
    }
}

/// Broadcast event bus for lifecycle events. Uses `tokio::sync::broadcast`
/// which supports multiple subscribers and backpressure.
#[derive(Clone)]
pub struct EventBus {
    sender: broadcast::Sender<LifecycleEvent>,
}

impl EventBus {
    /// Creates a new event bus with the default capacity.
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Self { sender }
    }

    /// Creates a new event bus with a custom capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Publishes an event to all subscribers.
    pub fn emit(&self, event: LifecycleEvent) {
        // Ignore error if there are no subscribers
        let _ = self.sender.send(event);
    }

    /// Subscribes to lifecycle events. Returns a receiver that will receive
    /// all future events from the time of subscription.
    pub fn subscribe(&self) -> broadcast::Receiver<LifecycleEvent> {
        self.sender.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

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
            dependents
                .entry(dep.as_str())
                .or_default()
                .push(name.as_str());
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
            remaining
                .keys()
                .filter(|n| !resolved.contains(*n))
                .collect::<Vec<_>>()
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
    port_mapper: PortMapper,
    health: HashMap<String, ServiceHealth>,
    manifest_name: String,
    /// Networks declared by the manifest passed to `up`, kept so `down`,
    /// `restart_service`, and `reap_and_restart` can (re)attach/detach
    /// services without needing the manifest passed back in every time.
    network_names: Vec<String>,
    /// Event bus for lifecycle transitions.
    events: EventBus,
}

impl LifecycleManager {
    pub fn new(manifest: &Manifest) -> Self {
        Self {
            runtime: RuntimeManager::new(),
            network: NetworkManager::new(),
            dns: DnsResolver::new(),
            port_mapper: PortMapper::new(),
            health: HashMap::new(),
            manifest_name: manifest.name.clone(),
            network_names: Vec::new(),
            events: EventBus::new(),
        }
    }

    /// Creates a new LifecycleManager in rootless mode, where networking
    /// uses slirp4netns instead of requiring root privileges.
    pub fn new_rootless(manifest: &Manifest) -> Self {
        Self {
            runtime: RuntimeManager::new(),
            network: NetworkManager::new().with_rootless(),
            dns: DnsResolver::new(),
            port_mapper: PortMapper::new(),
            health: HashMap::new(),
            manifest_name: manifest.name.clone(),
            network_names: Vec::new(),
            events: EventBus::new(),
        }
    }

    /// Returns a reference to the event bus for subscribing to lifecycle events.
    pub fn event_bus(&self) -> &EventBus {
        &self.events
    }

    pub async fn up(&mut self, manifest: &Manifest) -> Result<()> {
        tracing::info!("Bringing up environment: {}", manifest.name);

        for (name, net_config) in &manifest.networks {
            self.network
                .create_network(crate::network::NetworkConfig {
                    name: name.clone(),
                    driver: net_config.driver.clone(),
                    subnet: net_config.subnet.clone(),
                    gateway: net_config.gateway.clone(),
                    vni: net_config.vni,
                    peers: net_config.peers.clone(),
                })
                .await?;
        }
        self.network_names = manifest.networks.keys().cloned().collect();

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
                self.events.emit(LifecycleEvent::new(
                    EventType::Start,
                    name,
                    EventDetail::None,
                ));
                self.runtime.start_service(name, service).await?;
                if let Some(health) = self.health.get_mut(name) {
                    health.status = ServiceStatus::Running;
                    health.started_at = Some(Instant::now());
                }
                self.events.emit(LifecycleEvent::new(
                    EventType::Started,
                    name,
                    EventDetail::None,
                ));
                self.connect_service_networks(name).await;
                self.apply_port_mappings(name, service).await;
            }
        }

        tracing::info!("All services started for: {}", manifest.name);
        Ok(())
    }

    /// Attaches a just-started service to every network declared by the
    /// manifest and refreshes its DNS entry with the real allocated
    /// address. On Linux, an OCI service (which has a real containerd pid)
    /// gets a real veth into its network namespace; everything else
    /// (`process`/`wasm` services, or a non-Linux/unprivileged host) still
    /// gets a real, uniquely allocated address for bookkeeping/DNS, just
    /// without an actual attached device. Failures are logged, not
    /// propagated — a service that fails to attach to a network still keeps
    /// running with its placeholder DNS entry from `up`.
    async fn connect_service_networks(&mut self, name: &str) {
        if self.network_names.is_empty() {
            return;
        }
        let pid = self.runtime.list_pids().get(name).copied();
        for net_name in self.network_names.clone() {
            let result = match pid {
                Some(pid) => {
                    self.network
                        .connect_service_with_pid(name, &net_name, pid)
                        .await
                }
                None => self.network.connect_service(name, &net_name).await,
            };
            match result {
                Ok(ip) => {
                    self.dns.add_entry(name, &ip, None);
                    if let Some(ipv6) = self.network.assigned_ipv6(&net_name, name) {
                        self.dns.add_entry(name, ipv6, None);
                    }
                }
                Err(e) => tracing::warn!(
                    "failed to connect service '{name}' to network '{net_name}': {e:#}"
                ),
            }
        }
    }

    /// Sets up TCP port-forwarding proxies for every `PortMapping` declared
    /// by the service. For OCI services the target is the container's
    /// allocated IP on the first declared network; for `process`/`wasm`
    /// services the target is `127.0.0.1` (they share the host's network
    /// namespace). The proxy is a simple accept→forward loop — lightweight
    /// enough for local development traffic.
    async fn apply_port_mappings(&mut self, name: &str, service: &Service) {
        let ports = service.ports();
        if ports.is_empty() {
            return;
        }
        // Pick the container IP from the first declared network. Process/wasm
        // services that share the host network namespace will have an
        // allocated IP for bookkeeping/DNS but not a real separate namespace,
        // so we use 127.0.0.1 for those (they listen on the host directly).
        let target_ip = if self.network_names.is_empty() {
            "127.0.0.1".to_string()
        } else {
            self.network
                .assigned_ip(&self.network_names[0], name)
                .unwrap_or("127.0.0.1")
                .to_string()
        };

        // For process/wasm services (no real netns), the service is already
        // reachable on localhost — but port mappings still make sense if the
        // user wants to expose the port under a different host port number.
        // The proxy handles that transparently.
        if let Err(e) = self.port_mapper.apply(name, ports, &target_ip).await {
            tracing::warn!("failed to set up port mappings for '{name}': {e:#}");
        }
    }

    /// Detaches a service from every declared network (real veth teardown
    /// where one exists) — the inverse of `connect_service_networks`.
    async fn disconnect_service_networks(&mut self, name: &str) {
        for net_name in self.network_names.clone() {
            if let Err(e) = self.network.disconnect_service(name, &net_name).await {
                tracing::warn!(
                    "failed to disconnect service '{name}' from network '{net_name}': {e:#}"
                );
            }
        }
    }

    pub async fn down(&mut self) -> Result<()> {
        tracing::info!("Tearing down environment: {}", self.manifest_name);
        for name in self.health.keys().cloned().collect::<Vec<_>>() {
            self.events.emit(LifecycleEvent::new(
                EventType::Stop,
                &name,
                EventDetail::None,
            ));
        }
        self.port_mapper.stop_all();
        for name in self.health.keys().cloned().collect::<Vec<_>>() {
            self.disconnect_service_networks(&name).await;
        }
        self.runtime.stop_all().await?;
        for name in self.health.keys().cloned().collect::<Vec<_>>() {
            self.events.emit(LifecycleEvent::new(
                EventType::Stopped,
                &name,
                EventDetail::None,
            ));
        }
        self.dns.stop().await?;
        for net_name in self.network_names.clone() {
            if let Err(e) = self.network.delete_network(&net_name).await {
                tracing::warn!("failed to delete network '{net_name}': {e:#}");
            }
        }
        tracing::info!("Environment torn down: {}", self.manifest_name);
        Ok(())
    }

    pub async fn restart_service(&mut self, name: &str, manifest: &Manifest) -> Result<()> {
        tracing::info!("Restarting service: {name}");
        self.events.emit(LifecycleEvent::new(
            EventType::Restart,
            name,
            EventDetail::None,
        ));
        self.events.emit(LifecycleEvent::new(
            EventType::Stop,
            name,
            EventDetail::None,
        ));
        self.port_mapper.stop_service(name);
        self.disconnect_service_networks(name).await;
        self.runtime.stop_service(name).await?;
        self.events.emit(LifecycleEvent::new(
            EventType::Stopped,
            name,
            EventDetail::None,
        ));
        if let Some(service) = manifest.services.get(name) {
            self.events.emit(LifecycleEvent::new(
                EventType::Start,
                name,
                EventDetail::None,
            ));
            self.runtime.start_service(name, service).await?;
            if let Some(health) = self.health.get_mut(name) {
                health.restart_count += 1;
                health.status = ServiceStatus::Running;
                health.started_at = Some(Instant::now());
            }
            self.events.emit(LifecycleEvent::new(
                EventType::Started,
                name,
                EventDetail::None,
            ));
            self.connect_service_networks(name).await;
            self.apply_port_mappings(name, service).await;
        }
        Ok(())
    }

    /// Polls every running service for an unplanned exit and restarts any
    /// whose manifest `restart_policy` allows it — real crash recovery.
    /// Intended to be called periodically (e.g. from a supervising loop in
    /// the CLI); a single call only reaps whatever has already exited by
    /// the time it runs, it does not itself wait or loop.
    pub async fn reap_and_restart(&mut self, manifest: &Manifest) -> Result<Vec<String>> {
        let restarted = self.runtime.reap_and_restart(manifest).await?;
        for name in &restarted {
            self.events.emit(LifecycleEvent::new(
                EventType::Restart,
                name,
                EventDetail::None,
            ));
            if let Some(health) = self.health.get_mut(name) {
                health.restart_count += 1;
                health.status = ServiceStatus::Running;
                health.started_at = Some(Instant::now());
            }
            // The crashed process's netns (and with it, any veth peer) is
            // already gone by the time the kernel reaps it; this just drops
            // our now-stale bookkeeping entry so the reconnect below
            // doesn't short-circuit on a cached IP/veth from before the
            // crash.
            self.disconnect_service_networks(name).await;
            self.connect_service_networks(name).await;
            if let Some(service) = manifest.services.get(name) {
                self.apply_port_mappings(name, service).await;
            }
        }
        Ok(restarted)
    }

    /// Polls `OCIService.healthcheck` for every running OCI service and
    /// mirrors any resulting status transition into `ServiceHealth`.
    /// Intended to be called periodically, alongside `reap_and_restart`,
    /// from a supervising loop.
    pub async fn poll_healthchecks(&mut self, manifest: &Manifest) -> Result<Vec<String>> {
        let newly_failed = self.runtime.poll_healthchecks(manifest).await?;
        for name in self.health.keys().cloned().collect::<Vec<_>>() {
            if let Some(status) = self.runtime.get_status(&name) {
                if let Some(health) = self.health.get_mut(&name) {
                    let old_status = health.status.clone();
                    health.status = status.clone();
                    if old_status != *status {
                        match status {
                            ServiceStatus::HealthChecking => {
                                self.events.emit(LifecycleEvent::new(
                                    EventType::HealthFail,
                                    name,
                                    EventDetail::StatusChange(old_status, status.clone()),
                                ));
                            }
                            ServiceStatus::Running => {
                                self.events.emit(LifecycleEvent::new(
                                    EventType::HealthOk,
                                    name,
                                    EventDetail::StatusChange(old_status, status.clone()),
                                ));
                            }
                            ServiceStatus::Failed(msg) => {
                                self.events.emit(LifecycleEvent::new(
                                    EventType::Failed,
                                    name,
                                    EventDetail::Failure(msg.clone()),
                                ));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        Ok(newly_failed)
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

    /// Collects live resource usage stats for all running services.
    pub fn collect_stats(&self) -> Vec<crate::stats::ServiceStats> {
        let start_times: HashMap<String, std::time::Instant> = self
            .health
            .iter()
            .filter_map(|(name, h)| h.started_at.map(|t| (name.clone(), t)))
            .collect();
        crate::stats::collect_stats(self.runtime.list_services(), &start_times)
    }

    /// Pauses a running service, freezing its CPU and memory usage.
    pub async fn pause_service(&mut self, name: &str) -> Result<()> {
        tracing::info!("Pausing service: {name}");
        self.events.emit(LifecycleEvent::new(
            EventType::Pause,
            name,
            EventDetail::None,
        ));
        self.runtime.pause_service(name).await?;
        if let Some(health) = self.health.get_mut(name) {
            health.status = ServiceStatus::Paused;
        }
        Ok(())
    }

    /// Unpauses a paused service, resuming its execution.
    pub async fn unpause_service(&mut self, name: &str) -> Result<()> {
        tracing::info!("Unpausing service: {name}");
        self.events.emit(LifecycleEvent::new(
            EventType::Unpause,
            name,
            EventDetail::None,
        ));
        self.runtime.unpause_service(name).await?;
        if let Some(health) = self.health.get_mut(name) {
            health.status = ServiceStatus::Running;
        }
        Ok(())
    }

    /// Returns detailed inspect information for a named service, combining
    /// the manifest's static configuration with live runtime state (status,
    /// PID, uptime, restart count). Returns `None` if the service isn't
    /// known to the running environment.
    pub fn inspect(&self, name: &str, manifest: &Manifest) -> Option<InspectInfo> {
        let service = manifest.services.get(name)?;
        let health = self.health.get(name)?;
        let pids = self.runtime.list_pids();

        let mut info = InspectInfo {
            name: name.to_string(),
            service_type: match service {
                Service::OCI(_) => "oci".to_string(),
                Service::Wasm(_) => "wasm".to_string(),
                Service::Process(_) => "process".to_string(),
            },
            status: health.status.clone(),
            pid: pids.get(name).copied(),
            started_at_unix_millis: health.started_at.map(|instant| {
                let elapsed = instant.elapsed();
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default();
                now.saturating_sub(elapsed).as_millis() as u64
            }),
            restart_count: health.restart_count,
            command: None,
            image: None,
            path: None,
            environment: HashMap::new(),
            ports: Vec::new(),
            volumes: Vec::new(),
            working_dir: None,
            depends_on: service.depends_on().to_vec(),
            healthcheck: None,
            resources: None,
            security: None,
            logging: service.logging().clone(),
            env_file: service.env_file().clone(),
            secrets: service.secrets().as_ref().map(|s| {
                s.iter()
                    .map(|sec| SecretInfo {
                        name: sec.name.clone(),
                        target: sec.target.clone(),
                    })
                    .collect()
            }),
        };

        match service {
            Service::OCI(oci) => {
                info.image = Some(oci.image.clone());
                info.command = oci.command.clone();
                info.environment = oci.environment.clone();
                info.ports = oci.ports.clone();
                info.volumes = oci.volumes.clone();
                info.healthcheck = oci.healthcheck.clone();
                info.resources = oci.resources.clone();
                info.security = oci.security.clone();
            }
            Service::Wasm(wasm) => {
                info.path = Some(wasm.path.clone());
                info.environment = wasm.environment.clone();
                info.ports = wasm.ports.clone();
                info.resources = wasm.resources.clone();
                info.security = wasm.security.clone();
            }
            Service::Process(process) => {
                info.command = Some(process.command.clone());
                info.environment = process.environment.clone();
                info.ports = process.ports.clone();
                info.working_dir = process.working_dir.clone();
                info.resources = process.resources.clone();
                info.security = process.security.clone();
            }
        }

        Some(info)
    }

    pub async fn wait_for_signal(&self) -> Result<()> {
        tokio::signal::ctrl_c().await?;
        tracing::info!("Received shutdown signal");
        Ok(())
    }
}

/// Detailed information about a running service, combining manifest config
/// with live runtime state. Returned by [`LifecycleManager::inspect`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectInfo {
    pub name: String,
    pub service_type: String,
    pub status: ServiceStatus,
    pub pid: Option<u32>,
    pub started_at_unix_millis: Option<u64>,
    pub restart_count: u32,
    /// OCI image reference or process command (mutually exclusive by type).
    pub image: Option<String>,
    pub command: Option<Vec<String>>,
    /// Wasm module path (only set for `type: wasm` services).
    pub path: Option<std::path::PathBuf>,
    pub environment: HashMap<String, String>,
    pub ports: Vec<crate::manifest::PortMapping>,
    pub volumes: Vec<crate::manifest::VolumeMount>,
    pub working_dir: Option<std::path::PathBuf>,
    pub depends_on: Vec<String>,
    pub healthcheck: Option<crate::manifest::HealthCheck>,
    pub resources: Option<crate::manifest::ResourceLimits>,
    pub security: Option<crate::manifest::SecurityConfig>,
    pub logging: Option<crate::manifest::LoggingConfig>,
    pub env_file: Option<Vec<String>>,
    pub secrets: Option<Vec<SecretInfo>>,
}

/// Summary of a secret mount (name + target path), without exposing the
/// secret content itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretInfo {
    pub name: String,
    pub target: String,
}

#[cfg(test)]
mod topological_waves_tests {
    use super::*;
    use crate::manifest::{Manifest, ProcessService, Service};

    fn process_service(depends_on: &[&str]) -> Service {
        Service::Process(ProcessService {
            command: vec!["true".to_string()],
            ports: vec![],
            environment: HashMap::new(),
            working_dir: None,
            depends_on: depends_on.iter().map(|s| s.to_string()).collect(),
            resources: None,
            restart_policy: Default::default(),
            env_file: None,
            secrets: None,
            security: None,
            logging: None,
            healthcheck: None,
            profiles: vec![],
            configs: None,
            extends: None,
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
            logging: None,
            configs: HashMap::new(),
            secrets: HashMap::new(),
        }
    }

    #[test]
    fn linear_chain_orders_correctly() {
        // c depends on b, b depends on a -> waves must be [a], [b], [c]
        let manifest = manifest_with(&[("a", &[]), ("b", &["a"]), ("c", &["b"])]);
        let waves = topological_waves(&manifest).unwrap();
        assert_eq!(
            waves,
            vec![
                vec!["a".to_string()],
                vec!["b".to_string()],
                vec!["c".to_string()]
            ]
        );
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
