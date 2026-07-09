use std::fmt::Debug;

use anyhow::{bail, Result};

use super::wire_mysql::MysqlWireDriver;
use super::wire_postgres::PostgresWireDriver;
use super::wire_redis::{RedisWireDriver, RespFrame};
use super::QueryRow;

/// Identifies which database driver kind is in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DriverKind {
    Postgres,
    Mysql,
    Redis,
}

/// A transaction handle that can execute queries and commit/rollback.
/// Uses an enum to avoid dyn compatibility issues with native async fn in trait.
#[derive(Debug, Clone)]
pub enum WireTransaction {
    Postgres(PostgresWireDriver),
    Mysql(MysqlWireDriver),
    Redis(RedisWireDriver),
}

impl WireTransaction {
    /// Execute a query and return tabular results.
    pub async fn query(&mut self, sql: &str, params: &[serde_json::Value]) -> Result<QueryRow> {
        match self {
            WireTransaction::Postgres(d) => d.query(sql, params).await,
            WireTransaction::Mysql(d) => d.query(sql, params).await,
            WireTransaction::Redis(d) => d.query(sql, params).await,
        }
    }

    /// Execute a statement (INSERT/UPDATE/DELETE) and return affected row count.
    pub async fn execute(&mut self, sql: &str, params: &[serde_json::Value]) -> Result<u64> {
        match self {
            WireTransaction::Postgres(d) => d.execute(sql, params).await,
            WireTransaction::Mysql(d) => d.execute(sql, params).await,
            WireTransaction::Redis(d) => d.execute(sql, params).await,
        }
    }

    /// Commit the transaction.
    pub async fn commit(self) -> Result<()> {
        match self {
            WireTransaction::Postgres(d) => {
                d.simple_execute("COMMIT").await?;
                Ok(())
            }
            WireTransaction::Mysql(d) => {
                d.execute("COMMIT", &[]).await?;
                Ok(())
            }
            WireTransaction::Redis(d) => match d.send_and_read(&["EXEC"]).await? {
                RespFrame::Array(_) => Ok(()),
                RespFrame::Error(e) => bail!("EXEC failed: {}", e),
                other => bail!("unexpected EXEC response: {:?}", other),
            },
        }
    }

    /// Rollback the transaction.
    pub async fn rollback(self) -> Result<()> {
        match self {
            WireTransaction::Postgres(d) => {
                d.simple_execute("ROLLBACK").await?;
                Ok(())
            }
            WireTransaction::Mysql(d) => {
                d.execute("ROLLBACK", &[]).await?;
                Ok(())
            }
            WireTransaction::Redis(d) => match d.send_and_read(&["DISCARD"]).await? {
                RespFrame::Simple(s) if s == "OK" => Ok(()),
                RespFrame::Error(e) => bail!("DISCARD failed: {}", e),
                other => bail!("unexpected DISCARD response: {:?}", other),
            },
        }
    }
}

/// Unified driver enum replacing the per-database inherent impls.
/// Implementations handle connection, framing, and protocol-level concerns internally.
#[derive(Debug, Clone)]
pub enum WireDriver {
    Postgres(PostgresWireDriver),
    Mysql(MysqlWireDriver),
    Redis(RedisWireDriver),
}

impl WireDriver {
    /// Returns the driver kind.
    pub fn kind(&self) -> DriverKind {
        match self {
            WireDriver::Postgres(d) => d.kind(),
            WireDriver::Mysql(d) => d.kind(),
            WireDriver::Redis(d) => d.kind(),
        }
    }

    /// Whether this driver currently has an active connection.
    pub fn is_connected(&self) -> bool {
        match self {
            WireDriver::Postgres(d) => d.is_connected(),
            WireDriver::Mysql(d) => d.is_connected(),
            WireDriver::Redis(d) => d.is_connected(),
        }
    }

    /// Open a connection using the given host/port/database/credentials.
    pub async fn connect(
        &mut self,
        host: &str,
        port: u16,
        database: &str,
        username: &str,
        password: &str,
    ) -> Result<()> {
        match self {
            WireDriver::Postgres(d) => d.connect(host, port, database, username, password).await,
            WireDriver::Mysql(d) => d.connect(host, port, database, username, password).await,
            WireDriver::Redis(d) => d.connect(host, port, database, username, password).await,
        }
    }

    /// Execute a query and return tabular results.
    pub async fn query(&self, sql: &str, params: &[serde_json::Value]) -> Result<QueryRow> {
        match self {
            WireDriver::Postgres(d) => d.query(sql, params).await,
            WireDriver::Mysql(d) => d.query(sql, params).await,
            WireDriver::Redis(d) => d.query(sql, params).await,
        }
    }

    /// Execute a statement (INSERT/UPDATE/DELETE) and return affected row count.
    pub async fn execute(&self, sql: &str, params: &[serde_json::Value]) -> Result<u64> {
        match self {
            WireDriver::Postgres(d) => d.execute(sql, params).await,
            WireDriver::Mysql(d) => d.execute(sql, params).await,
            WireDriver::Redis(d) => d.execute(sql, params).await,
        }
    }

    /// Start a new transaction.
    pub async fn begin_transaction(&self) -> Result<WireTransaction> {
        match self {
            WireDriver::Postgres(d) => {
                d.simple_execute("BEGIN").await?;
                Ok(WireTransaction::Postgres(d.clone()))
            }
            WireDriver::Mysql(d) => {
                d.execute("BEGIN", &[]).await?;
                Ok(WireTransaction::Mysql(d.clone()))
            }
            WireDriver::Redis(d) => match d.send_and_read(&["MULTI"]).await? {
                RespFrame::Simple(s) if s == "OK" => Ok(WireTransaction::Redis(d.clone())),
                RespFrame::Error(e) => bail!("MULTI failed: {}", e),
                other => bail!("unexpected MULTI response: {:?}", other),
            },
        }
    }

    /// Liveness probe.
    pub async fn ping(&self) -> Result<()> {
        match self {
            WireDriver::Postgres(d) => d.ping().await,
            WireDriver::Mysql(d) => d.ping().await,
            WireDriver::Redis(d) => d.ping().await,
        }
    }
}

/// Helper to create a driver by kind.
pub fn box_driver(kind: DriverKind) -> WireDriver {
    match kind {
        DriverKind::Postgres => WireDriver::Postgres(PostgresWireDriver::new()),
        DriverKind::Mysql => WireDriver::Mysql(MysqlWireDriver::new()),
        DriverKind::Redis => WireDriver::Redis(RedisWireDriver::new()),
    }
}
