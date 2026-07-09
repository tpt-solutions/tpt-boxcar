use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::drivers::{DatabaseConfig, DriverKind, QueryRow, WireDriver};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitParams {
    pub values: Vec<serde_json::Value>,
}

/// WIT binding facade that delegates to any WireDriver implementation.
pub struct TetherWit {
    inner: WireDriver,
    /// Identity of the calling Wasm module/service, presented via
    /// `TETHER_CALLER_ID` (set by Origin's manifest service key). Used to
    /// resolve capability-scoped credentials in `connect_with_config`.
    caller_id: Option<String>,
}

impl TetherWit {
    /// Create a new TetherWit backed by the given driver.
    pub fn new(driver: WireDriver) -> Self {
        Self {
            inner: driver,
            caller_id: None,
        }
    }

    /// Create a TetherWit with a PostgresWireDriver (backward compatible).
    pub fn new_postgres() -> Self {
        Self {
            inner: WireDriver::Postgres(crate::drivers::PostgresWireDriver::new()),
            caller_id: None,
        }
    }

    /// Create a TetherWit with a MysqlWireDriver.
    pub fn new_mysql() -> Self {
        Self {
            inner: WireDriver::Mysql(crate::drivers::MysqlWireDriver::new()),
            caller_id: None,
        }
    }

    /// Create a TetherWit with a RedisWireDriver.
    pub fn new_redis() -> Self {
        Self {
            inner: WireDriver::Redis(crate::drivers::RedisWireDriver::new()),
            caller_id: None,
        }
    }

    /// Sets the caller identity used to resolve scoped credentials.
    pub fn with_caller(mut self, caller_id: impl Into<String>) -> Self {
        self.caller_id = Some(caller_id.into());
        self
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
        self.inner
            .connect(host, port, database, username, password)
            .await
    }

    /// Connect using DatabaseConfig (resolves credentials internally,
    /// scoped to this instance's caller identity if one was set via
    /// `with_caller`).
    pub async fn connect_with_config(&mut self, config: &DatabaseConfig) -> Result<()> {
        let resolved = config.credentials.resolve_for(self.caller_id.as_deref())?;
        self.inner
            .connect(
                &config.host,
                config.port,
                &config.database,
                &resolved.username,
                &resolved.password,
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

    fn require_redis(&self) -> Result<()> {
        if self.inner.kind() == DriverKind::Redis {
            Ok(())
        } else {
            anyhow::bail!(
                "kv operations require a connected key-value driver (use RedisWireDriver)"
            )
        }
    }

    pub async fn kv_get(&self, key: &str) -> Result<Option<String>> {
        self.require_redis()?;
        let row = self
            .inner
            .query("GET", &[serde_json::Value::String(key.to_string())])
            .await?;
        Ok(row
            .values
            .first()
            .and_then(|v| v.as_str().map(String::from)))
    }

    pub async fn kv_set(&self, key: &str, value: &str) -> Result<()> {
        self.require_redis()?;
        self.inner
            .execute(
                "SET",
                &[
                    serde_json::Value::String(key.to_string()),
                    serde_json::Value::String(value.to_string()),
                ],
            )
            .await?;
        Ok(())
    }

    pub async fn kv_del(&self, key: &str) -> Result<u64> {
        self.require_redis()?;
        self.inner
            .execute("DEL", &[serde_json::Value::String(key.to_string())])
            .await
    }

    pub async fn kv_expire(&self, key: &str, ttl_secs: u64) -> Result<bool> {
        self.require_redis()?;
        let affected = self
            .inner
            .execute(
                "EXPIRE",
                &[
                    serde_json::Value::String(key.to_string()),
                    serde_json::Value::Number(ttl_secs.into()),
                ],
            )
            .await?;
        Ok(affected > 0)
    }
}
