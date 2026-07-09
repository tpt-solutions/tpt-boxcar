pub mod wire;
pub mod wire_mysql;
pub mod wire_postgres;
pub mod wire_redis;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub use wire::{box_driver, DriverKind, WireDriver, WireTransaction};
pub use wire_mysql::MysqlWireDriver;
pub use wire_postgres::PostgresWireDriver;
pub use wire_redis::{RedisWireDriver, RespFrame};

use crate::auth::ScopedCredentials;
use crate::tls::TlsConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub credentials: ScopedCredentials,
    #[serde(default)]
    pub tls: Option<TlsConfig>,
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
    #[serde(default = "default_min_connections")]
    pub min_connections: u32,
    #[serde(default = "default_connect_timeout")]
    pub connect_timeout_secs: u64,
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout_secs: u64,
}

fn default_max_connections() -> u32 {
    20
}
fn default_min_connections() -> u32 {
    5
}
fn default_connect_timeout() -> u64 {
    10
}
fn default_idle_timeout() -> u64 {
    300
}

impl DatabaseConfig {
    pub fn connection_string(&self, caller: Option<&str>) -> Result<String> {
        let resolved = self
            .credentials
            .resolve_for(caller)
            .context("failed to resolve scoped credentials")?;
        Ok(format!(
            "postgres://{}:{}@{}:{}/{}",
            resolved.username, resolved.password, self.host, self.port, self.database
        ))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryRow {
    pub columns: Vec<String>,
    pub values: Vec<serde_json::Value>,
}
