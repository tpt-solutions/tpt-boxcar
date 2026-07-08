# AGENTS.md

## Repository layout

TPT Boxcar is a monorepo of five products. Two workspace systems govern builds:

- **Rust workspace** (root `Cargo.toml`): 8 crates. Run `cargo build --workspace` from root.
- **Go workspace** (`go.work`): 4 modules — `tether/control-plane`, `scope/backend`, `frontier/control-plane`, `mcp-server`. Run `go build ./...` from root.

### Crate names (for `-p` flags)

| Crate | Package name |
|-------|-------------|
| `origin/core` | `tpt-origin-core` |
| `origin/cli` | `tpt-origin-cli` |
| `tether/proxy` | `tpt-tether-proxy` |
| `scope/agent` | `tpt-scope-agent` |
| `chisel/core` | `tpt-chisel-core` |
| `chisel/cli` | `tpt-chisel-cli` |
| `frontier/proxy` | `tpt-frontier-proxy` |
| `frontier/plugin-sdk` | `tpt-frontier-plugin-sdk` |

## Gotchas that will waste your time

### eBPF workspace is deliberately separate

`scope/ebpf/` has its own `Cargo.toml` workspace and is **not** a member of the root workspace. The `probe/` subdirectory requires nightly Rust + `bpfel-unknown-none` target (see `scope/ebpf/probe/rust-toolchain.toml`). Building `probe/` happens automatically via `aya-build` in `scope/ebpf/loader/build.rs` — never invoke `cargo build` on it directly.

The `ebpf` feature on `tpt-scope-agent` is **off by default**. Plain `cargo build --workspace` never touches the eBPF path. Only enable it when working on Scope's eBPF probes on Linux.

### Containerd feature is Linux-only

`tpt-origin-core` has `containerd` on by default, but the dependency is gated behind `cfg(target_os = "linux")`. On Windows/macOS the crate compiles but containerd integration is inert. The containerd integration tests in CI run as a separate job with `--features containerd -- --ignored --test-threads=1` and need a live containerd daemon.

### Protobuf files are reference, not codegen

`frontier/proto/` contains `.proto` definitions, but no code generation step exists. The Go control plane uses hand-written structs (e.g., `internal/store/store.go`) that mirror the proto messages. Don't look for `buf.gen.yaml` or `prost-build` — there isn't one.

## Build commands

```bash
# Everything (Rust + Go)
cargo build --workspace
go build ./...

# Single Rust crate
cargo build -p tpt-frontier-proxy

# Single Go module
go build ./frontier/control-plane/...

# Scope dashboard (Vite + React)
cd scope/dashboard && npm install && npm run build

# Origin GUI (Tauri)
cd origin/gui && npm install && npm run tauri build
```

## Test commands

```bash
# All Rust
cargo test --workspace

# Single crate
cargo test -p tpt-origin-core

# Single test by name
cargo test -p tpt-origin-core dns_resolves_service_name

# Go (all three modules from root)
go test ./tether/control-plane/... ./scope/backend/... ./frontier/control-plane/...

# Scope dashboard
cd scope/dashboard && npm test           # vitest
cd scope/dashboard && npm run test:e2e   # Playwright
```

## Lint & format (CI order: fmt → build → test → clippy)

```bash
cargo fmt --all -- --check   # CI checks this first
cargo clippy --workspace -- -D warnings
golangci-lint run ./...
cd scope/dashboard && npm run lint
cd origin/gui && npm run lint
```

Rust formatting: `max_width = 100`, `tab_spaces = 4`, edition 2021. Clippy `msrv = "1.75.0"`.

## Architecture shortcuts

- **Tether data plane** (`tether/proxy/src/`): hand-written wire-protocol drivers implementing `WireDriver` trait. No sqlx/redis-rs — all protocol framing is manual.
- **Frontier data plane** (`frontier/proxy/src/`): Hyper HTTP listener, Wasmtime plugin loader with hot-reload. Config from `/var/run/frontier/config.json` written by the Go control plane.
- **Scope ingest** (`scope/backend/`): OTLP gRPC on 4317, HTTP on 4318. ClickHouse native protocol via `clickhouse-go/v2`.
- **Chisel AI** (`chisel/core/src/ai/`): `LlmProvider` trait with Ollama/Claude/OpenAI backends. Backend toggled via `ai.backend` in `chisel.yaml`.
- **Origin** (`origin/core/src/`): manifest parser (YAML/TOML), containerd + Wasmtime runtime, eBPF networking (Linux only via Aya).
