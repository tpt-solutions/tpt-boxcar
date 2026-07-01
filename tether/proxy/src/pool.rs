use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Mutex, Semaphore};
use tracing::{debug, info, warn};

use crate::drivers::{DatabaseConfig, WireDriver};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolConfig {
    #[serde(default = "default_min_size")]
    pub min_size: u32,
    #[serde(default = "default_max_size")]
    pub max_size: u32,
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout_secs: u64,
    #[serde(default = "default_acquire_timeout")]
    pub acquire_timeout_secs: u64,
}

fn default_min_size() -> u32 {
    2
}
fn default_max_size() -> u32 {
    10
}
fn default_idle_timeout() -> u64 {
    300
}
fn default_acquire_timeout() -> u64 {
    30
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            min_size: default_min_size(),
            max_size: default_max_size(),
            idle_timeout_secs: default_idle_timeout(),
            acquire_timeout_secs: default_acquire_timeout(),
        }
    }
}

#[allow(dead_code)]
struct PooledConnection {
    driver: WireDriver,
    created_at: Instant,
    last_used: Instant,
}

pub struct ConnectionPool {
    config: PoolConfig,
    db_config: DatabaseConfig,
    connections: Arc<DashMap<u64, Mutex<PooledConnection>>>,
    idle_connections: mpsc::Sender<u64>,
    idle_receiver: Arc<Mutex<mpsc::Receiver<u64>>>,
    semaphore: Arc<Semaphore>,
    next_id: std::sync::atomic::AtomicU64,
    idle_count: std::sync::atomic::AtomicUsize,
}

impl ConnectionPool {
    pub fn new(config: PoolConfig, db_config: DatabaseConfig) -> Self {
        let max = config.max_size as usize;
        let (idle_tx, idle_rx) = mpsc::channel(max);

        Self {
            config,
            db_config,
            connections: Arc::new(DashMap::new()),
            idle_connections: idle_tx,
            idle_receiver: Arc::new(Mutex::new(idle_rx)),
            semaphore: Arc::new(Semaphore::new(max)),
            next_id: std::sync::atomic::AtomicU64::new(0),
            idle_count: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// Create a pool with a pre-built driver factory.
    /// Each time a new connection is needed, `make_driver()` is called.
    pub fn new_with_factory(
        config: PoolConfig,
        db_config: DatabaseConfig,
        _make_driver: impl Fn() -> WireDriver + Send + Sync + 'static,
    ) -> Self {
        let max = config.max_size as usize;
        let (idle_tx, idle_rx) = mpsc::channel(max);

        Self {
            config,
            db_config,
            connections: Arc::new(DashMap::new()),
            idle_connections: idle_tx,
            idle_receiver: Arc::new(Mutex::new(idle_rx)),
            semaphore: Arc::new(Semaphore::new(max)),
            next_id: std::sync::atomic::AtomicU64::new(0),
            idle_count: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    pub async fn initialize(&self) -> Result<()> {
        let min = self.config.min_size as usize;
        info!(min_size = min, "initializing connection pool");

        for _ in 0..min {
            let conn = self.create_connection().await?;
            let id = self
                .next_id
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            self.connections.insert(id, Mutex::new(conn));
            let _ = self.idle_connections.send(id).await;
            self.idle_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }

        info!(total = self.connections.len(), "pool initialized");
        Ok(())
    }

    async fn create_connection(&self) -> Result<PooledConnection> {
        let mut driver = WireDriver::Postgres(crate::drivers::PostgresWireDriver::new());
        let creds = &self.db_config.credentials;
        let username = creds.username().context("failed to resolve username")?;
        let password = creds.password().context("failed to resolve password")?;

        driver
            .connect(
                &self.db_config.host,
                self.db_config.port,
                &self.db_config.database,
                &username,
                &password,
            )
            .await?;

        let now = Instant::now();
        Ok(PooledConnection {
            driver,
            created_at: now,
            last_used: now,
        })
    }

    pub async fn acquire(&self) -> Result<PooledConnectionGuard<'_>> {
        let timeout = Duration::from_secs(self.config.acquire_timeout_secs);

        let permit = tokio::time::timeout(timeout, self.semaphore.clone().acquire_owned())
            .await
            .context("acquire timeout")?
            .context("semaphore closed")?;

        if let Ok(id) = self.idle_receiver.lock().await.try_recv() {
            if let Some(entry) = self.connections.get(&id) {
                let mut conn = entry.lock().await;
                let idle_duration = conn.last_used.elapsed();
                if idle_duration < Duration::from_secs(self.config.idle_timeout_secs) {
                    // Ping the connection before handing it to the caller; drop it if dead.
                    match conn.driver.ping().await {
                        Ok(()) => {
                            conn.last_used = Instant::now();
                            self.idle_count.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                            return Ok(PooledConnectionGuard {
                                id,
                                pool: self,
                                _permit: permit,
                            });
                        }
                        Err(e) => {
                            warn!(id, err = %e, "idle connection failed ping, dropping");
                            drop(conn);
                            self.connections.remove(&id);
                            self.idle_count.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                } else {
                    drop(conn);
                    self.connections.remove(&id);
                    self.idle_count.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                    debug!(id, "removed stale idle connection");
                }
            }
        }

        let conn = self.create_connection().await?;
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.connections.insert(id, Mutex::new(conn));
        debug!(id, "created new connection");

        Ok(PooledConnectionGuard {
            id,
            pool: self,
            _permit: permit,
        })
    }

    pub fn return_connection(&self, id: u64) {
        if self.connections.contains_key(&id) {
            let _ = self.idle_connections.try_send(id);
            self.idle_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    pub fn stats(&self) -> PoolStats {
        PoolStats {
            total: self.connections.len() as u32,
            max: self.config.max_size,
            idle: self.idle_count.load(std::sync::atomic::Ordering::Relaxed) as u32,
        }
    }

    pub async fn shutdown(&self) {
        info!(
            count = self.connections.len(),
            "shutting down connection pool"
        );
        self.connections.clear();
        self.idle_count.store(0, std::sync::atomic::Ordering::Relaxed);
    }
}

pub struct PooledConnectionGuard<'a> {
    id: u64,
    pool: &'a ConnectionPool,
    _permit: tokio::sync::OwnedSemaphorePermit,
}

impl<'a> PooledConnectionGuard<'a> {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub async fn driver<F, R>(&self, f: F) -> Result<R>
    where
        F: std::future::Future<Output = Result<R>>,
    {
        let entry = self
            .pool
            .connections
            .get(&self.id)
            .context("connection not found")?;
        let mut conn = entry.lock().await;
        conn.last_used = Instant::now();
        drop(conn);
        f.await
    }

    /// Access the underlying WireDriver.
    pub async fn with_driver<F, R>(&self, f: F) -> Result<R>
    where
        F: std::future::Future<Output = Result<R>>,
    {
        let entry = self
            .pool
            .connections
            .get(&self.id)
            .context("connection not found")?;
        let mut conn = entry.lock().await;
        conn.last_used = Instant::now();
        drop(conn);
        f.await
    }
}

impl<'a> Drop for PooledConnectionGuard<'a> {
    fn drop(&mut self) {
        self.pool.return_connection(self.id);
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PoolStats {
    pub total: u32,
    pub max: u32,
    pub idle: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_config_defaults() {
        let config = PoolConfig::default();
        assert_eq!(config.min_size, 2);
        assert_eq!(config.max_size, 10);
        assert_eq!(config.idle_timeout_secs, 300);
        assert_eq!(config.acquire_timeout_secs, 30);
    }

    #[test]
    fn test_pool_config_serde_roundtrip() {
        let config = PoolConfig {
            min_size: 1,
            max_size: 50,
            idle_timeout_secs: 600,
            acquire_timeout_secs: 60,
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: PoolConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.max_size, 50);
        assert_eq!(deserialized.min_size, 1);
        assert_eq!(deserialized.idle_timeout_secs, 600);
    }

    #[test]
    fn test_pool_stats_creation() {
        let stats = PoolStats {
            total: 5,
            max: 10,
            idle: 3,
        };
        assert_eq!(stats.total, 5);
        assert_eq!(stats.max, 10);
        assert_eq!(stats.idle, 3);
    }
}