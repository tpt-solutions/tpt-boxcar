use anyhow::Result;

use crate::wit::TetherWit;

pub struct ComponentizeP2 {
    wit: TetherWit,
}

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

impl Default for ComponentizeP2 {
    fn default() -> Self {
        Self::new()
    }
}

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