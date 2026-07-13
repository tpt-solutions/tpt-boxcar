# tpt-origin-core

[![Crates.io](https://img.shields.io/crates/v/tpt-origin-core.svg)](https://crates.io/crates/tpt-origin-core)
[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](../../LICENSE)

Core runtime library for [TPT Origin](../../origin) — the unified local sandbox that runs OCI containers and Wasm modules side-by-side without a Docker daemon.

## What it does

- **Manifest parsing** — load `boxcar.yaml` / `origin.toml` service definitions via serde
- **OCI container lifecycle** — containerd-backed create/start/stop/remove (Linux)
- **Wasm module lifecycle** — Wasmtime-backed instantiation with WASI Preview 2
- **eBPF networking** — virtual network fabric via Aya (Linux), per-service DNS names
- **Local DNS** — resolves `<service>.local` names across the sandbox
- **Secrets** — mounts env vars and file secrets into services at startup

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
tpt-origin-core = "0.1"
```

```rust
use tpt_origin_core::{Manifest, RuntimeManager};

let manifest = Manifest::from_file("origin.toml").await?;
let mut rt = RuntimeManager::new(&manifest).await?;
rt.start_all().await?;
```

## Feature flags

| Flag | Default | Description |
|------|---------|-------------|
| `containerd` | on | Enable containerd OCI backend (Linux only at runtime) |

## Part of TPT Boxcar

See the [monorepo README](../../README.md) for the full product suite.

## License

Apache-2.0
