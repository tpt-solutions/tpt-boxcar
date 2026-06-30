use std::time::Duration;

use anyhow::Result;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, error, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthConfig {
    #[serde(default = "default_ping_interval")]
    pub ping_interval_secs: u64,
    #[serde(default = "default_max_failures")]
    pub max_failures: u32,
    #[serde(default = "default_reconnect_base")]
    pub reconnect_base_ms: u64,
    #[serde(default = "default_reconnect_max")]
    pub reconnect_max_ms: u64,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_ping_interval() -> u64 {
    30
}
fn default_max_failures() -> u32 {
    3
}
fn default_reconnect_base() -> u64 {
    1000
}
fn default_reconnect_max() -> u64 {
    30000
}
fn default_enabled() -> bool {
    true
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            ping_interval_secs: default_ping_interval(),
            max_failures: default_max_failures(),
            reconnect_base_ms: default_reconnect_base(),
            reconnect_max_ms: default_reconnect_max(),
            enabled: default_enabled(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
}

#[derive(Debug, Clone)]
pub struct ConnectionHealth {
    pub status: HealthStatus,
    pub consecutive_failures: u32,
    pub last_success: Option<std::time::Instant>,
    pub last_failure: Option<std::time::Instant>,
    pub total_pings: u64,
    pub total_failures: u64,
}

impl Default for ConnectionHealth {
    fn default() -> Self {
        Self {
            status: HealthStatus::Healthy,
            consecutive_failures: 0,
            last_success: None,
            last_failure: None,
            total_pings: 0,
            total_failures: 0,
        }
    }
}

pub trait HealthCheck: Send + Sync + 'static {
    fn check(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send>>;
}

pub struct HealthMonitor {
    config: HealthConfig,
    health: DashMap<u64, ConnectionHealth>,
    evict_tx: mpsc::Sender<u64>,
}

impl HealthMonitor {
    pub fn new(config: HealthConfig) -> Self {
        let (evict_tx, _) = mpsc::channel(64);

        Self {
            config,
            health: DashMap::new(),
            evict_tx,
        }
    }

    pub fn register(&self, id: u64) {
        self.health.insert(id, ConnectionHealth::default());
        debug!(id, "registered connection for health monitoring");
    }

    pub fn unregister(&self, id: u64) {
        self.health.remove(&id);
    }

    pub fn record_success(&self, id: u64) {
        if let Some(mut entry) = self.health.get_mut(&id) {
            entry.consecutive_failures = 0;
            entry.last_success = Some(std::time::Instant::now());
            entry.total_pings += 1;
            entry.status = HealthStatus::Healthy;
        }
    }

    pub fn record_failure(&self, id: u64) {
        if let Some(mut entry) = self.health.get_mut(&id) {
            entry.consecutive_failures += 1;
            entry.last_failure = Some(std::time::Instant::now());
            entry.total_pings += 1;
            entry.total_failures += 1;

            if entry.consecutive_failures >= self.config.max_failures {
                entry.status = HealthStatus::Unhealthy;
                warn!(
                    id,
                    failures = entry.consecutive_failures,
                    "connection marked unhealthy"
                );
            } else if entry.consecutive_failures >= self.config.max_failures / 2 {
                entry.status = HealthStatus::Degraded;
            }
        }
    }

    pub fn get_health(&self, id: u64) -> Option<ConnectionHealth> {
        self.health.get(&id).map(|e| e.clone())
    }

    pub fn unhealthy_ids(&self) -> Vec<u64> {
        self.health
            .iter()
            .filter(|e| e.status == HealthStatus::Unhealthy)
            .map(|e| *e.key())
            .collect()
    }

    pub fn backoff_duration(&self, consecutive_failures: u32) -> Duration {
        let base = self.config.reconnect_base_ms as f64;
        let max = self.config.reconnect_max_ms as f64;
        let delay = (base * 2f64.powi(consecutive_failures as i32 - 1)).min(max);
        Duration::from_millis(delay as u64)
    }

    pub fn start_ping_loop<F>(&self, health_check: F)
    where
        F: HealthCheck + 'static,
    {
        let interval = Duration::from_secs(self.config.ping_interval_secs);
        let evict_tx = self.evict_tx.clone();

        let ids: Vec<u64> = self.health.iter().map(|e| *e.key()).collect();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;

                for &id in &ids {
                    match health_check.check().await {
                        Ok(()) => {
                            debug!(id, "health check passed");
                        }
                        Err(e) => {
                            warn!(id, error = %e, "health check failed");
                            if evict_tx.send(id).await.is_err() {
                                error!("evict channel closed");
                                return;
                            }
                        }
                    }
                }
            }
        });
    }
}

#[derive(Debug, Clone)]
pub struct PostgresHealthCheck;

impl HealthCheck for PostgresHealthCheck {
    fn check(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send>> {
        Box::pin(async { Ok(()) })
    }
}
