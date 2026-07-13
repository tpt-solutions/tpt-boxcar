# tpt-frontier-plugin-sdk

[![Crates.io](https://img.shields.io/crates/v/tpt-frontier-plugin-sdk.svg)](https://crates.io/crates/tpt-frontier-plugin-sdk)
[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](../../LICENSE)

Rust SDK for authoring Wasm plugins for the **TPT Frontier** edge gateway and service mesh.

## What it does

Provides the host ABI that Frontier exposes to plugins, letting you:

- Intercept and modify HTTP requests and responses
- Read and write request/response headers
- Short-circuit requests (e.g. return a cached response or auth error)
- Log structured events back to the host
- Share state across plugin invocations via host-managed storage

## Usage

Add to your plugin's `Cargo.toml` and compile to `wasm32-wasi`:

```toml
[dependencies]
tpt-frontier-plugin-sdk = "0.1"

[lib]
crate-type = ["cdylib"]
```

```rust
use tpt_frontier_plugin_sdk::{handle_request, Request, Response};

#[handle_request]
fn my_plugin(req: &mut Request) -> Option<Response> {
    if req.header("x-skip") == Some("true") {
        return Some(Response::new(204));
    }
    req.set_header("x-processed-by", "my-plugin");
    None   // continue to upstream
}
```

Build with:

```bash
cargo build --target wasm32-wasi --release
```

Then register the `.wasm` file via the Frontier control plane REST API or `boxcar.yaml`.

## TypeScript SDK

A TypeScript equivalent is available at `frontier/typescript-sdk/` in the [monorepo](https://github.com/tpt-boxcar/tpt-boxcar).

## Part of TPT Boxcar

See the [monorepo README](../../README.md) for the full product suite.

## License

Apache-2.0
