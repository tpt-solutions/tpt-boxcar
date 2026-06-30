# Plugin Authoring Guide

## Overview

TPT Frontier plugins are WebAssembly modules that extend the proxy's request/response pipeline. Plugins are loaded at runtime via Wasmtime and communicate through a stable ABI.

## Prerequisites

- Rust 1.75+ with `wasm32-wasi` target: `rustup target add wasm32-wasi`
- For TypeScript plugins: Node.js 18+, `@tpt-cloud-native/frontier-plugin-sdk`

## Rust Plugin

### 1. Create the Project

```bash
cargo new --lib my-plugin
cd my-plugin
```

### 2. Add Dependencies

```toml
[dependencies]
tpt-frontier-plugin-sdk = "0.1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

### 3. Implement the Plugin

```rust
use std::collections::HashMap;
use tpt_frontier_plugin_sdk::abi::{FilterResult, HttpRequest, HttpResponse, PluginFilter};

pub struct MyPlugin;

impl PluginFilter for MyPlugin {
    fn on_request(&self, request: &HttpRequest) -> FilterResult {
        if request.headers.contains_key("x-debug") {
            tracing::info!("debug mode detected");
        }
        FilterResult::Continue
    }

    fn on_response(&self, _request: &HttpRequest, response: &HttpResponse) -> FilterResult {
        FilterResult::Continue
    }
}
```

### 4. Build

```bash
cargo build --target wasm32-wasi --release
```

The output `.wasm` file is ready to load into Frontier.

### 5. Test Locally

```bash
cargo test
```

Use the SDK's test utilities to mock host functions and validate filter behavior.

## TypeScript Plugin

### 1. Initialize

```bash
mkdir my-ts-plugin && cd my-ts-plugin
npm init -y
npm install @tpt-cloud-native/frontier-plugin-sdk
npm install -D typescript
```

### 2. Implement

```typescript
import {
  FrontierPlugin,
  HostFunctions,
  HttpRequest,
  FilterResult,
  PluginContext,
  continueResult,
} from "@tpt-cloud-native/frontier-plugin-sdk";

class MyPlugin implements FrontierPlugin {
  private host: HostFunctions | null = null;

  init(host: HostFunctions): void {
    this.host = host;
    host.log(1, "my plugin loaded");
  }

  onRequest(request: HttpRequest, ctx: PluginContext): FilterResult {
    return continueResult();
  }

  onResponse(request: HttpRequest, response: HttpResponse, ctx: PluginContext): FilterResult {
    return continueResult();
  }

  shutdown(): void {}
}

export default new MyPlugin();
```

### 3. Build

```bash
npx tsc
```

Then compile to Wasm using the AssemblyScript compiler or a Wasm-capable TypeScript toolchain.

## Hot-Loading

Place the compiled `.wasm` file in Frontier's plugin directory. The proxy watches for file changes:

```
plugins/
  my_plugin.wasm
```

Modify the `.wasm` file in place. Frontier detects the change, recompiles the module, and swaps it in with zero dropped connections.

### Verify Hot-Reload

```bash
# Copy new version
cp target/wasm32-wasi/release/my_plugin.wasm plugins/

# Check logs for hot-reload message
# [INFO] hot-reloaded plugin 'my_plugin' from plugins/my_plugin.wasm
```

## Configuration

Plugins receive configuration via the `PluginConfig` struct (Rust) or `host.getConfig()` (TypeScript). Configuration is set in the proxy's config file:

```yaml
plugins:
  - name: my_plugin
    path: plugins/my_plugin.wasm
    enabled: true
    config:
      api_key: "secret"
      max_retries: "3"
```

## Debugging

### Rust Plugins

Use `tracing` crate for logging. Logs appear in Frontier's log stream:

```rust
tracing::info!("processing request to {}", request.path);
tracing::warn!("rate limit approaching");
```

### TypeScript Plugins

Use `host.log(level, message)`:

```typescript
host.log(0, "debug info");   // Debug
host.log(1, "info message"); // Info
host.log(2, "warning");      // Warn
host.log(3, "error");        // Error
```

## Resource Limits

Plugins run in a sandboxed Wasm environment with configurable resource limits:

- **Memory**: Default 64MB, configurable per-plugin
- **Fuel**: Instruction budget per request
- **Epoch**: Timeout via epoch interruption

These are set in the proxy config and enforced by the Wasmtime runtime.

## Security

- Plugins cannot access the filesystem, network, or host OS directly
- All host interaction goes through the defined ABI (`host_*` functions)
- Plugins are isolated from each other in separate Wasm instances
- Resource limits prevent runaway plugins from affecting the proxy
