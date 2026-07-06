pub mod manifest;
pub mod runtime;
pub mod network;
pub mod dns;
pub mod lifecycle;
#[cfg(all(target_os = "linux", feature = "containerd"))]
pub mod containerd;

use lifecycle::{LifecycleManager, ServiceHealthDto};
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

    pub async fn up(&mut self, manifest: &Manifest) -> anyhow::Result<()> {
        self.lifecycle.up(manifest).await
    }

    pub async fn down(&mut self) -> anyhow::Result<()> {
        self.lifecycle.down().await
    }

    pub async fn restart_service(&mut self, name: &str, manifest: &Manifest) -> anyhow::Result<()> {
        self.lifecycle.restart_service(name, manifest).await
    }

    pub fn status(&self, name: &str) -> Option<ServiceHealthDto> {
        self.lifecycle.get_service_status(name).map(ServiceHealthDto::from)
    }

    pub fn list(&self) -> Vec<ServiceHealthDto> {
        self.lifecycle
            .list_services()
            .values()
            .map(ServiceHealthDto::from)
            .collect()
    }

    pub fn service_pids(&self) -> std::collections::HashMap<String, u32> {
        self.lifecycle.service_pids()
    }
}
