use std::fmt::Debug;

use anyhow::Result;
use async_trait::async_trait;

use super::QueryRow;

/// Identifies which database driver kind is in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DriverKind {
    Postgres,
    Mysql,
    Redis,
}

/// A transaction handle that can execute queries and commit/rollback.
#[async_trait]
pub trait WireTransaction: Send + Sync {
    async fn query(&mut self, sql: &str, params: &[serde_json::Value]) -> Result<QueryRow>;
    async fn execute(&mut self, sql: &str, params: &[serde_json::Value]) -> Result<u64>;
    async fn commit(self: Box<Self>) -> Result<()>;
    async fn rollback(self: Box<Self>) -> Result<()>;
}

/// Unified driver trait replacing the per-database inherent impls.
/// Implementations handle connection, framing, and protocol-level concerns internally.
#[async_trait]
pub trait WireDriver: Send + Sync + Debug {
    /// Returns the driver kind.
    fn kind(&self) -> DriverKind;

    /// Whether this driver currently has an active connection.
    fn is_connected(&self) -> bool;

    /// Open a connection using the given host/port/database/credentials.
    async fn connect(
        &mut self,
        host: &str,
        port: u16,
        database: &str,
        username: &str,
        password: &str,
    ) -> Result<()>;

    /// Execute a query and return tabular results.
    async fn query(&self, sql: &str, params: &[serde_json::Value]) -> Result<QueryRow>;

    /// Execute a statement (INSERT/UPDATE/DELETE) and return affected row count.
    async fn execute(&self, sql: &str, params: &[serde_json::Value]) -> Result<u64>;

    /// Start a new transaction.
    async fn begin_transaction(&self) -> Result<Box<dyn WireTransaction>>;

    /// Liveness probe.
    async fn ping(&self) -> Result<()>;
}

/// Helper to box-erase any WireDriver.
pub fn box_driver<D: WireDriver + 'static>(driver: D) -> Box<dyn WireDriver> {
    Box::new(driver)
}
