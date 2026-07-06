# tether-wit-guest-demo

Example Wasm guest that calls Tether's `tpt:tether` WIT interfaces
(`data` query/execute, `kv` get/set). Demonstrates the guest side of the
component-model bindings defined in [`tether/wit/tether.wit`](../../wit/tether.wit)
and is the fixture loaded by `tether/proxy/tests/component_test.rs`.

Deliberately **not** a workspace member (see its `Cargo.toml`): it depends on
`wit-bindgen`, a guest-only codegen crate that no host crate in this repo
should pull in, and it only ever targets `wasm32-wasip1`.

## Build

```sh
rustup target add wasm32-wasip1   # one-time
cargo build --target wasm32-wasip1
```

This produces a core Wasm module (not yet a component) at
`target/wasm32-wasip1/debug/tether_wit_guest_demo.wasm`.

## Turn it into a component

`wit-bindgen`'s `generate!` macro targets Preview 1, so the compiled module
needs adapting into an actual Component Model component before `wasmtime`
can load it, via [`wasm-tools`](https://github.com/bytecodealliance/wasm-tools)
and the `wasi_snapshot_preview1` adapter:

```sh
cargo install wasm-tools

# Download the adapter matching your wasmtime version (25.x here) from
# https://github.com/bytecodealliance/wasmtime/releases — grab
# `wasi_snapshot_preview1.reactor.wasm` from the matching release assets.

wasm-tools component new \
  target/wasm32-wasip1/debug/tether_wit_guest_demo.wasm \
  --adapt wasi_snapshot_preview1=wasi_snapshot_preview1.reactor.wasm \
  -o tether_wit_guest_demo.component.wasm
```

## Run the integration test against it

```sh
TETHER_WIT_GUEST_COMPONENT=$(pwd)/tether_wit_guest_demo.component.wasm \
  cargo test -p tpt-tether-proxy --test component_test -- --ignored
```

The test wires the component up to an **unconnected** Redis-kind driver on
the host side (no live Redis server required) — it exercises the guest →
host wiring and confirms errors come back as typed `tether-error` values
rather than panics, not full driver correctness (that's covered separately
by the wire-driver integration tests).
