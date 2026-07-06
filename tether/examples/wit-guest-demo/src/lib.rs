//! Example Wasm guest demonstrating the `tpt:tether` WIT interfaces.
//!
//! Exercises `data::query`/`execute` and `kv::set`/`get` against whatever
//! `WireDriver` the host has connected. Intended as a manual smoke test and as
//! the fixture loaded by `tether/proxy/tests/component_test.rs`.

wit_bindgen::generate!({
    world: "tether-guest",
    path: "../../wit",
});

use tpt::tether::{data, kv};

struct GuestImpl;

impl Guest for GuestImpl {
    /// Runs the demo: sets a kv key, reads it back, and runs a query,
    /// returning a human-readable summary for the caller to print/assert on.
    fn run() -> String {
        let mut lines = Vec::new();

        match kv::set("wit-guest-demo:greeting", "hello from wasm") {
            Ok(()) => lines.push("kv::set ok".to_string()),
            Err(e) => lines.push(format!("kv::set error: {e:?}")),
        }

        match kv::get("wit-guest-demo:greeting") {
            Ok(value) => lines.push(format!("kv::get -> {value:?}")),
            Err(e) => lines.push(format!("kv::get error: {e:?}")),
        }

        match data::query("SELECT 1", &[]) {
            Ok(row) => lines.push(format!(
                "data::query -> columns={:?} values={}",
                row.columns,
                row.values.len()
            )),
            Err(e) => lines.push(format!("data::query error: {e:?}")),
        }

        lines.join("\n")
    }
}

export!(GuestImpl);
