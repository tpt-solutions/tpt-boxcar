# tpt-origin

[![Crates.io](https://img.shields.io/crates/v/tpt-origin.svg)](https://crates.io/crates/tpt-origin)
[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](../../LICENSE)

CLI for **TPT Origin** — a unified local sandbox that runs OCI containers and Wasm modules side-by-side without a Docker daemon.

## Install

```bash
cargo install tpt-origin
```

## Quick start

```bash
# Initialise a new sandbox in ./demo
tpt origin init --dir demo
cd demo

# Start all services defined in origin.toml
tpt origin up

# Check service status
tpt origin status

# Stream logs from a service
tpt origin logs web

# Tear everything down
tpt origin down
```

## Commands

| Command | Description |
|---------|-------------|
| `tpt origin init` | Scaffold a new `origin.toml` and directory layout |
| `tpt origin up` | Start all services in the manifest |
| `tpt origin down` | Stop and remove all services |
| `tpt origin status` | Show running service status |
| `tpt origin logs <svc>` | Tail logs from a service |
| `tpt origin exec <svc>` | Exec a command inside a running service |
| `tpt origin build` | Build OCI images declared in the manifest |

## Part of TPT Boxcar

See the [monorepo README](../../README.md) for the full product suite.

## License

Apache-2.0
