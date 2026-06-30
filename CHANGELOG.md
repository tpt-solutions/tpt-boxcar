# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0] - 2026-06-30

### Added

- **TPT Origin** — Unified local sandbox for running OCI containers and Wasm side by side
- **TPT Tether** — State and connection proxy for Wasm workloads with Go control plane
- **TPT Scope** — Hybrid eBPF observability providing zero-code distributed tracing
- **TPT Chisel** — Automated image distiller and Wasm migration analyzer with AI-powered insights
- **TPT Frontier** — Wasm-native edge service mesh and API gateway with plugin SDK

### Infrastructure

- Rust workspace with Tokio async runtime
- Go control plane and core engine components
- React + TypeScript dashboards for Scope and Origin GUIs
- Docker-based development environment
- CI/CD pipeline with `cargo test`, `cargo clippy`, `go test`, and `golangci-lint`

[1.0.0]: https://github.com/tpt-solutions/tpt-cloud-native/releases/tag/v1.0.0
