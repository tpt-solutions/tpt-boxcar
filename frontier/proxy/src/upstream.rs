use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::RwLock;
use tracing::{debug, info};

use crate::loadbalancer::BackendEndpoint;

#[derive(Debug, Clone)]
pub struct UpstreamConfig {
    pub name: String,
    pub endpoints: Vec<BackendEndpoint>,
    pub connect_timeout: std::time::Duration,
    pub request_timeout: std::time::Duration,
    pub max_connections: usize,
}

pub struct UpstreamPool {
    config: UpstreamConfig,
    healthy_endpoints: RwLock<Vec<BackendEndpoint>>,
    unhealthy_endpoints: RwLock<Vec<SocketAddr>>,
}

impl UpstreamPool {
    pub fn new(config: UpstreamConfig) -> Self {
        let endpoints = config.endpoints.clone();
        info!(
            "creating upstream pool '{}' with {} endpoints",
            config.name,
            endpoints.len()
        );
        Self {
            config,
            healthy_endpoints: RwLock::new(endpoints),
            unhealthy_endpoints: RwLock::new(Vec::new()),
        }
    }

    pub async fn get_healthy_endpoints(&self) -> Vec<BackendEndpoint> {
        self.healthy_endpoints.read().await.clone()
    }

    pub async fn mark_unhealthy(&self, addr: SocketAddr) {
        let mut healthy = self.healthy_endpoints.write().await;
        healthy.retain(|ep| ep.addr != addr);
        let mut unhealthy = self.unhealthy_endpoints.write().await;
        unhealthy.push(addr);
        debug!("marked {} as unhealthy", addr);
    }

    pub async fn mark_healthy(&self, endpoint: BackendEndpoint) {
        let mut unhealthy = self.unhealthy_endpoints.write().await;
        unhealthy.retain(|addr| *addr != endpoint.addr);
        let mut healthy = self.healthy_endpoints.write().await;
        if !healthy.iter().any(|ep| ep.addr == endpoint.addr) {
            healthy.push(endpoint.clone());
            debug!("marked {} as healthy", endpoint.addr);
        }
    }

    pub async fn health_check(&self) -> Result<()> {
        let endpoints = self.get_healthy_endpoints().await;
        let mut newly_unhealthy = Vec::new();

        for endpoint in &endpoints {
            match tokio::time::timeout(
                self.config.connect_timeout,
                tokio::net::TcpStream::connect(endpoint.addr),
            )
            .await
            {
                Ok(Ok(_)) => {
                    debug!("health check passed for {}", endpoint.addr);
                }
                Ok(Err(e)) => {
                    debug!("health check failed for {}: {}", endpoint.addr, e);
                    newly_unhealthy.push(endpoint.addr);
                }
                Err(_) => {
                    debug!("health check timeout for {}", endpoint.addr);
                    newly_unhealthy.push(endpoint.addr);
                }
            }
        }

        for addr in newly_unhealthy {
            self.mark_unhealthy(addr).await;
        }

        Ok(())
    }

    pub fn config(&self) -> &UpstreamConfig {
        &self.config
    }
}

pub struct UpstreamManager {
    pools: RwLock<Vec<Arc<UpstreamPool>>>,
}

impl UpstreamManager {
    pub fn new() -> Self {
        Self {
            pools: RwLock::new(Vec::new()),
        }
    }

    pub async fn add_pool(&self, pool: Arc<UpstreamPool>) {
        let mut pools = self.pools.write().await;
        info!("registering upstream pool '{}'", pool.config().name);
        pools.push(pool);
    }

    pub async fn find_pool(&self, name: &str) -> Option<Arc<UpstreamPool>> {
        let pools = self.pools.read().await;
        pools.iter().find(|p| p.config().name == name).cloned()
    }

    pub async fn run_health_checks(&self) -> Result<()> {
        let pools = self.pools.read().await;
        for pool in pools.iter() {
            pool.health_check().await?;
        }
        Ok(())
    }

    /// Atomically replace all pools (hot-reload support).
    pub async fn reload(&self, new_pools: Vec<Arc<UpstreamPool>>) {
        let mut pools = self.pools.write().await;
        *pools = new_pools;
        info!("upstream manager reloaded: {} pools", pools.len());
    }

    /// Returns a snapshot of current pool names.
    pub async fn pool_names(&self) -> Vec<String> {
        let pools = self.pools.read().await;
        pools.iter().map(|p| p.config().name.clone()).collect()
    }
}
