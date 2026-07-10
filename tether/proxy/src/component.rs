//! Host-side wasmtime Component Model bindings for `tether/wit/tether.wit`.
//!
//! Generates the `tpt:tether/data` and `tpt:tether/kv` host traits via
//! `wasmtime::component::bindgen!` and implements them on top of the existing
//! [`WireDriver`] enum. This is an adapter layer only — `drivers/wire.rs` and
//! the individual wire-protocol drivers are untouched.

use crate::drivers::{
    DriverKind, QueryRow as InternalQueryRow, Subscription, WireDriver, WireTransaction,
};

wasmtime::component::bindgen!({
    path: "../wit",
    world: "tether-guest",
    async: true,
});

use tpt::tether::pubsub::Message as WitMessage;
use tpt::tether::types::{QueryRow as WitQueryRow, TetherError, Value as WitValue};

/// Host state passed to the wasmtime `Store` for an instantiated guest
/// component. Wraps a single connected [`WireDriver`].
pub struct WitHostState {
    driver: WireDriver,
    open_transaction: Option<WireTransaction>,
    /// Connection parameters used to open dedicated pub/sub connections
    /// (Redis puts a subscribed connection into a restricted mode, so it
    /// can't reuse `driver`'s own connection — see `drivers/pubsub.rs`).
    /// `None` means `pubsub::subscription::open` will fail with
    /// `not-connected`, which is correct for hosts that never called
    /// `with_connection_info` (e.g. the unconnected-driver smoke test in
    /// `component_test.rs`).
    conn_info: Option<(String, u16, String, String)>,
    /// Open subscriptions, indexed by resource rep (slab allocator: `None`
    /// marks a freed slot, reused on the next `open`).
    subscriptions: Vec<Option<Subscription>>,
}

impl WitHostState {
    pub fn new(driver: WireDriver) -> Self {
        Self {
            driver,
            open_transaction: None,
            conn_info: None,
            subscriptions: Vec::new(),
        }
    }

    /// Records host/port/credentials so `pubsub::subscription::open` can
    /// open its own dedicated connection later. Should be called with the
    /// same parameters used to `connect()` the wrapped `driver`.
    pub fn with_connection_info(
        mut self,
        host: impl Into<String>,
        port: u16,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        self.conn_info = Some((host.into(), port, username.into(), password.into()));
        self
    }
}

fn to_wit_value(value: &serde_json::Value) -> WitValue {
    match value {
        serde_json::Value::Null => WitValue::Null,
        serde_json::Value::Bool(b) => WitValue::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                WitValue::Int(i)
            } else if let Some(f) = n.as_f64() {
                WitValue::Float(f)
            } else {
                WitValue::Text(n.to_string())
            }
        }
        serde_json::Value::String(s) => WitValue::Text(s.clone()),
        other => WitValue::Text(other.to_string()),
    }
}

fn from_wit_value(value: &WitValue) -> serde_json::Value {
    match value {
        WitValue::Null => serde_json::Value::Null,
        WitValue::Bool(b) => serde_json::Value::Bool(*b),
        WitValue::Int(i) => serde_json::Value::Number((*i).into()),
        WitValue::Float(f) => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        WitValue::Text(s) => serde_json::Value::String(s.clone()),
    }
}

fn to_wit_row(row: InternalQueryRow) -> WitQueryRow {
    WitQueryRow {
        columns: row.columns,
        values: row.values.iter().map(to_wit_value).collect(),
    }
}

fn to_wit_error(err: anyhow::Error) -> TetherError {
    TetherError::DriverError(err.to_string())
}

#[wasmtime::component::__internal::async_trait]
impl tpt::tether::data::HostTransaction for WitHostState {
    async fn query(
        &mut self,
        _self_: wasmtime::component::Resource<tpt::tether::data::Transaction>,
        sql: String,
        params: Vec<WitValue>,
    ) -> Result<WitQueryRow, TetherError> {
        let tx = self
            .open_transaction
            .as_mut()
            .ok_or(TetherError::NotConnected)?;
        let params: Vec<serde_json::Value> = params.iter().map(from_wit_value).collect();
        tx.query(&sql, &params)
            .await
            .map(to_wit_row)
            .map_err(to_wit_error)
    }

    async fn execute(
        &mut self,
        _self_: wasmtime::component::Resource<tpt::tether::data::Transaction>,
        sql: String,
        params: Vec<WitValue>,
    ) -> Result<u64, TetherError> {
        let tx = self
            .open_transaction
            .as_mut()
            .ok_or(TetherError::NotConnected)?;
        let params: Vec<serde_json::Value> = params.iter().map(from_wit_value).collect();
        tx.execute(&sql, &params).await.map_err(to_wit_error)
    }

    async fn commit(
        &mut self,
        _self_: wasmtime::component::Resource<tpt::tether::data::Transaction>,
    ) -> Result<(), TetherError> {
        let tx = self
            .open_transaction
            .take()
            .ok_or(TetherError::NotConnected)?;
        tx.commit().await.map_err(to_wit_error)
    }

    async fn rollback(
        &mut self,
        _self_: wasmtime::component::Resource<tpt::tether::data::Transaction>,
    ) -> Result<(), TetherError> {
        let tx = self
            .open_transaction
            .take()
            .ok_or(TetherError::NotConnected)?;
        tx.rollback().await.map_err(to_wit_error)
    }

    async fn drop(
        &mut self,
        _rep: wasmtime::component::Resource<tpt::tether::data::Transaction>,
    ) -> wasmtime::Result<()> {
        Ok(())
    }
}

#[wasmtime::component::__internal::async_trait]
impl tpt::tether::data::Host for WitHostState {
    async fn query(
        &mut self,
        sql: String,
        params: Vec<WitValue>,
    ) -> Result<WitQueryRow, TetherError> {
        let params: Vec<serde_json::Value> = params.iter().map(from_wit_value).collect();
        self.driver
            .query(&sql, &params)
            .await
            .map(to_wit_row)
            .map_err(to_wit_error)
    }

    async fn execute(&mut self, sql: String, params: Vec<WitValue>) -> Result<u64, TetherError> {
        let params: Vec<serde_json::Value> = params.iter().map(from_wit_value).collect();
        self.driver
            .execute(&sql, &params)
            .await
            .map_err(to_wit_error)
    }

    async fn begin_transaction(
        &mut self,
    ) -> Result<wasmtime::component::Resource<tpt::tether::data::Transaction>, TetherError> {
        let tx = self
            .driver
            .begin_transaction()
            .await
            .map_err(to_wit_error)?;
        self.open_transaction = Some(tx);
        // A single in-flight transaction per store is all v1 needs; the guest
        // never inspects the resource handle's value, only threads it through
        // query/execute/commit/rollback.
        Ok(wasmtime::component::Resource::new_own(0))
    }
}

#[wasmtime::component::__internal::async_trait]
impl tpt::tether::kv::Host for WitHostState {
    async fn get(&mut self, key: String) -> Result<Option<String>, TetherError> {
        require_redis(&self.driver)?;
        match self
            .driver
            .query("GET", &[serde_json::Value::String(key)])
            .await
        {
            Ok(row) => Ok(row
                .values
                .first()
                .and_then(|v| v.as_str().map(String::from))),
            Err(e) => Err(to_wit_error(e)),
        }
    }

    async fn set(&mut self, key: String, value: String) -> Result<(), TetherError> {
        require_redis(&self.driver)?;
        self.driver
            .execute(
                "SET",
                &[
                    serde_json::Value::String(key),
                    serde_json::Value::String(value),
                ],
            )
            .await
            .map(|_| ())
            .map_err(to_wit_error)
    }

    async fn del(&mut self, key: String) -> Result<u64, TetherError> {
        require_redis(&self.driver)?;
        self.driver
            .execute("DEL", &[serde_json::Value::String(key)])
            .await
            .map_err(to_wit_error)
    }

    async fn expire(&mut self, key: String, ttl_secs: u64) -> Result<bool, TetherError> {
        require_redis(&self.driver)?;
        let affected = self
            .driver
            .execute(
                "EXPIRE",
                &[
                    serde_json::Value::String(key),
                    serde_json::Value::Number(ttl_secs.into()),
                ],
            )
            .await
            .map_err(to_wit_error)?;
        Ok(affected > 0)
    }
}

#[wasmtime::component::__internal::async_trait]
impl tpt::tether::pubsub::HostSubscription for WitHostState {
    async fn open(
        &mut self,
        channels: Vec<String>,
    ) -> Result<wasmtime::component::Resource<tpt::tether::pubsub::Subscription>, TetherError>
    {
        require_redis(&self.driver)?;
        let (host, port, username, password) =
            self.conn_info.clone().ok_or(TetherError::NotConnected)?;
        let sub = Subscription::open(&host, port, &username, &password, &channels)
            .await
            .map_err(to_wit_error)?;

        // Slab allocation: reuse a freed slot (from a dropped subscription)
        // if one exists, otherwise grow.
        let rep = if let Some(idx) = self.subscriptions.iter().position(|s| s.is_none()) {
            self.subscriptions[idx] = Some(sub);
            idx
        } else {
            self.subscriptions.push(Some(sub));
            self.subscriptions.len() - 1
        };
        Ok(wasmtime::component::Resource::new_own(rep as u32))
    }

    async fn subscribe(
        &mut self,
        self_: wasmtime::component::Resource<tpt::tether::pubsub::Subscription>,
        channels: Vec<String>,
    ) -> Result<(), TetherError> {
        let sub = self
            .subscriptions
            .get(self_.rep() as usize)
            .and_then(|s| s.as_ref())
            .ok_or(TetherError::NotConnected)?;
        sub.subscribe_more(&channels).await.map_err(to_wit_error)
    }

    async fn unsubscribe(
        &mut self,
        self_: wasmtime::component::Resource<tpt::tether::pubsub::Subscription>,
        channels: Vec<String>,
    ) -> Result<(), TetherError> {
        let sub = self
            .subscriptions
            .get(self_.rep() as usize)
            .and_then(|s| s.as_ref())
            .ok_or(TetherError::NotConnected)?;
        sub.unsubscribe(&channels).await.map_err(to_wit_error)
    }

    async fn poll(
        &mut self,
        self_: wasmtime::component::Resource<tpt::tether::pubsub::Subscription>,
    ) -> Result<Option<WitMessage>, TetherError> {
        let sub = self
            .subscriptions
            .get(self_.rep() as usize)
            .and_then(|s| s.as_ref())
            .ok_or(TetherError::NotConnected)?;
        Ok(sub.poll().await.map(|m| WitMessage {
            channel: m.channel,
            payload: m.payload,
        }))
    }

    async fn drop(
        &mut self,
        rep: wasmtime::component::Resource<tpt::tether::pubsub::Subscription>,
    ) -> wasmtime::Result<()> {
        if let Some(slot) = self.subscriptions.get_mut(rep.rep() as usize) {
            // Dropping the `Subscription` aborts its background reader task.
            *slot = None;
        }
        Ok(())
    }
}

#[wasmtime::component::__internal::async_trait]
impl tpt::tether::pubsub::Host for WitHostState {
    async fn publish(&mut self, channel: String, payload: String) -> Result<u64, TetherError> {
        require_redis(&self.driver)?;
        self.driver
            .execute(
                "PUBLISH",
                &[
                    serde_json::Value::String(channel),
                    serde_json::Value::String(payload),
                ],
            )
            .await
            .map_err(to_wit_error)
    }
}

// The `types` interface only declares shared type definitions (no
// functions), but bindgen still generates a `Host` trait for it since it's
// `use`d by both `data` and `kv` — nothing to implement.
impl tpt::tether::types::Host for WitHostState {}

fn require_redis(driver: &WireDriver) -> Result<(), TetherError> {
    if driver.kind() == DriverKind::Redis {
        Ok(())
    } else {
        Err(TetherError::UnsupportedOperation(
            "kv operations require a Redis-backed driver".to_string(),
        ))
    }
}
