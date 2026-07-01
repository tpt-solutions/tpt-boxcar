use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::drivers::{DatabaseConfig, DriverKind, WireDriver, QueryRow};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitParams {
    pub values: Vec<serde_json::Value>,
}

/// WIT binding facade that delegates to any WireDriver implementation.
pub struct TetherWit {
    inner: WireDriver,
}

impl TetherWit {
    /// Create a new TetherWit backed by the given driver.
    pub fn new(driver: WireDriver) -> Self {
        Self { inner: driver }
    }

    /// Create a TetherWit with a PostgresWireDriver (backward compatible).
    pub fn new_postgres() -> Self {
        Self {
            inner: WireDriver::Postgres(crate::drivers::PostgresWireDriver::new()),
        }
    }

    /// Create a TetherWit with a MysqlWireDriver.
    pub fn new_mysql() -> Self {
        Self {
            inner: WireDriver::Mysql(crate::drivers::MysqlWireDriver::new()),
        }
    }

    /// Create a TetherWit with a RedisWireDriver.
    pub fn new_redis() -> Self {
        Self {
            inner: WireDriver::Redis(crate::drivers::RedisWireDriver::new()),
        }
    }

    pub fn driver_kind(&self) -> DriverKind {
        self.inner.kind()
    }

    pub fn is_connected(&self) -> bool {
        self.inner.is_connected()
    }

    pub async fn connect(
        &mut self,
        host: &str,
        port: u16,
        database: &str,
        username: &str,
        password: &str,
    ) -> Result<()> {
        self.inner.connect(host, port, database, username, password).await
    }

    /// Connect using DatabaseConfig (resolves credentials internally).
    pub async fn connect_with_config(&mut self, config: &DatabaseConfig) -> Result<()> {
        let username = config.credentials.username()?;
        let password = config.credentials.password()?;
        self.inner
            .connect(
                &config.host,
                config.port,
                &config.database,
                &username,
                &password,
            )
            .await
    }

    pub async fn query(&self, sql: &str, params: WitParams) -> Result<QueryRow> {
        self.inner.query(sql, &params.values).await
    }

    pub async fn execute(&self, sql: &str, params: WitParams) -> Result<u64> {
        self.inner.execute(sql, &params.values).await
    }

    pub async fn begin_transaction(&self) -> Result<crate::drivers::WireTransaction> {
        self.inner.begin_transaction().await
    }

    pub async fn ping(&self) -> Result<()> {
        self.inner.ping().await
    }

    pub async fn kv_get(&self, _key: &str) -> Result<Option<String>> {
        anyhow::bail!("kv_get requires a connected key-value driver (use RedisWireDriver)")
    }

    pub async fn kv_set(&self, _key: &str, _value: &str) -> Result<()> {
        anyhow::bail!("kv_set requires a connected key-value driver (use RedisWireDriver)")
    }
}