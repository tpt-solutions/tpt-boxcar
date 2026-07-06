//! End-to-end guest -> host test for the `tpt:tether` WIT bindings.
//!
//! Loads the pre-built, componentized `tether-wit-guest-demo` example (see
//! `tether/examples/wit-guest-demo/README.md` for how to build+adapt it into
//! a component) and calls its `run` export, which itself calls back into the
//! `data`/`kv` host interfaces implemented in `tpt_tether_proxy::component`.
//!
//! Requires a real network build step (wasm32-wasip1 target + `wasm-tools
//! component new` + the wasi_snapshot_preview1 adapter) that isn't wired into
//! `cargo test` by default, so this is `#[ignore]`d like the other
//! infrastructure-dependent integration tests in this crate (see
//! `wire_driver_integration.rs`). Run with:
//!
//! ```sh
//! TETHER_WIT_GUEST_COMPONENT=/path/to/tether-wit-guest-demo.component.wasm \
//!     cargo test -p tpt-tether-proxy --test component_test -- --ignored
//! ```

use tpt_tether_proxy::component::{TetherGuest, WitHostState};
use tpt_tether_proxy::drivers::{box_driver, DriverKind};
use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store};

#[tokio::test]
#[ignore = "requires a pre-built+adapted wasm32-wasip1 component; see module docs"]
async fn guest_run_round_trips_through_host_kv_and_data_interfaces() -> anyhow::Result<()> {
    let component_path = std::env::var("TETHER_WIT_GUEST_COMPONENT")
        .expect("set TETHER_WIT_GUEST_COMPONENT to the built component's .wasm path");

    let mut config = Config::new();
    config.async_support(true);
    let engine = Engine::new(&config)?;

    let component = Component::from_file(&engine, &component_path)?;

    let mut linker = Linker::new(&engine);
    TetherGuest::add_to_linker(&mut linker, |state: &mut WitHostState| state)?;

    // Unconnected Redis driver: no live server needed for this smoke test.
    // The guest's kv/data calls are expected to surface as typed
    // `tether-error`s (not panics), which is what this test actually verifies
    // — that the guest -> host wiring round-trips errors correctly end to end.
    let driver = box_driver(DriverKind::Redis);
    let mut store = Store::new(&engine, WitHostState::new(driver));

    let instance = TetherGuest::instantiate_async(&mut store, &component, &linker).await?;
    let summary = instance.call_run(&mut store).await?;

    assert!(summary.contains("kv::set"));
    assert!(summary.contains("kv::get"));
    assert!(summary.contains("data::query"));

    Ok(())
}
