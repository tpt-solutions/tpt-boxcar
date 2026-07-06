use anyhow::Result;

use crate::wit::TetherWit;

#[deprecated(
    since = "0.2.0",
    note = "superseded by the real wasmtime component instantiation path in `crate::component`; \
            this JSON-serializing shim predates the actual WIT bindings and is kept only for \
            existing callers during the transition"
)]
pub struct ComponentizeP2 {
    wit: TetherWit,
}

#[allow(deprecated)]
impl ComponentizeP2 {
    pub fn new() -> Self {
        Self {
            wit: TetherWit::new_postgres(),
        }
    }

    pub fn with_driver(driver: crate::drivers::WireDriver) -> Self {
        Self {
            wit: TetherWit::new(driver),
        }
    }

    pub fn as_wit(&self) -> &TetherWit {
        &self.wit
    }

    pub fn as_wit_mut(&mut self) -> &mut TetherWit {
        &mut self.wit
    }
}

#[allow(deprecated)]
impl Default for ComponentizeP2 {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(deprecated)]
pub async fn handle_query(
    component: &ComponentizeP2,
    sql: String,
    params: Vec<serde_json::Value>,
) -> Result<String> {
    let result = component
        .wit
        .query(&sql, crate::wit::WitParams { values: params })
        .await?;
    Ok(serde_json::to_string(&result)?)
}

#[allow(deprecated)]
pub async fn handle_execute(
    component: &ComponentizeP2,
    sql: String,
    params: Vec<serde_json::Value>,
) -> Result<u64> {
    component
        .wit
        .execute(&sql, crate::wit::WitParams { values: params })
        .await
}