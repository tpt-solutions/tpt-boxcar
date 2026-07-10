#[cfg(all(target_os = "linux", feature = "containerd"))]
pub mod build;
#[cfg(all(target_os = "linux", feature = "containerd"))]
pub mod containerd;
pub mod dns;
pub mod dockerfile;
pub mod envfile;
pub mod lifecycle;
pub mod logging;
pub mod manifest;
pub mod network;
pub mod portmap;
pub mod reslimit;
pub mod runtime;
pub mod security;
pub mod stats;

pub use lifecycle::EventBus;

use lifecycle::{InspectInfo, LifecycleManager, ServiceHealthDto};
use manifest::Manifest;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Embeddable facade over Origin's sandbox lifecycle, for programmatic
/// callers that want to drive Origin without assembling
/// `RuntimeManager`/`NetworkManager`/`DnsResolver`/`LifecycleManager`
/// themselves. `origin/cli` is itself the first consumer of this facade.
///
/// Deliberately does not expose [`LifecycleManager::wait_for_signal`] (it
/// blocks on ctrl_c, a CLI-only concern with no meaning for an embedder).
pub struct Origin {
    lifecycle: LifecycleManager,
}

impl Origin {
    pub fn new(manifest: &Manifest) -> Self {
        Self {
            lifecycle: LifecycleManager::new(manifest),
        }
    }

    /// Creates a new Origin instance in rootless mode, where networking
    /// uses slirp4netns for unprivileged connectivity.
    pub fn new_rootless(manifest: &Manifest) -> Self {
        Self {
            lifecycle: LifecycleManager::new_rootless(manifest),
        }
    }

    /// Returns a reference to the event bus for subscribing to lifecycle events.
    /// Subscribers receive real-time notifications of service state changes.
    pub fn event_bus(&self) -> &EventBus {
        self.lifecycle.event_bus()
    }

    pub async fn up(&mut self, manifest: &Manifest) -> anyhow::Result<()> {
        self.lifecycle.up(manifest).await
    }

    pub async fn down(&mut self) -> anyhow::Result<()> {
        self.lifecycle.down().await
    }

    pub async fn restart_service(&mut self, name: &str, manifest: &Manifest) -> anyhow::Result<()> {
        self.lifecycle.restart_service(name, manifest).await
    }

    /// Reaps any service that exited on its own and, per its manifest
    /// `restart_policy`, restarts it. Call this periodically (e.g. from a
    /// supervising loop) — a single call only catches whatever has already
    /// exited by the time it runs.
    pub async fn reap_and_restart(&mut self, manifest: &Manifest) -> anyhow::Result<Vec<String>> {
        self.lifecycle.reap_and_restart(manifest).await
    }

    /// Runs due `OCIService.healthcheck` probes and updates service status
    /// accordingly. Call this periodically alongside `reap_and_restart`.
    pub async fn poll_healthchecks(&mut self, manifest: &Manifest) -> anyhow::Result<Vec<String>> {
        self.lifecycle.poll_healthchecks(manifest).await
    }

    pub fn status(&self, name: &str) -> Option<ServiceHealthDto> {
        self.lifecycle
            .get_service_status(name)
            .map(ServiceHealthDto::from)
    }

    pub fn list(&self) -> Vec<ServiceHealthDto> {
        self.lifecycle
            .list_services()
            .values()
            .map(ServiceHealthDto::from)
            .collect()
    }

    /// Returns detailed inspect information for a named service, combining
    /// the manifest's static configuration with live runtime state.
    pub fn inspect(&self, name: &str, manifest: &Manifest) -> Option<InspectInfo> {
        self.lifecycle.inspect(name, manifest)
    }

    /// Collects live resource usage stats (CPU, memory, network) for all
    /// running services.
    pub fn collect_stats(&self) -> Vec<stats::ServiceStats> {
        self.lifecycle.collect_stats()
    }

    /// Pauses a running service, freezing its CPU and memory usage.
    pub async fn pause_service(&mut self, name: &str) -> anyhow::Result<()> {
        self.lifecycle.pause_service(name).await
    }

    /// Unpauses a paused service, resuming its execution.
    pub async fn unpause_service(&mut self, name: &str) -> anyhow::Result<()> {
        self.lifecycle.unpause_service(name).await
    }

    pub fn service_pids(&self) -> std::collections::HashMap<String, u32> {
        self.lifecycle.service_pids()
    }
}
