# tpt-chisel-core

[![Crates.io](https://img.shields.io/crates/v/tpt-chisel-core.svg)](https://crates.io/crates/tpt-chisel-core)
[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](../../LICENSE)

Core library for **TPT Chisel** — an automated OCI image distiller and Wasm migrator with LLM-assisted analysis.

## What it does

- **Image analysis** — scans OCI image layers, builds a dependency graph, generates an SBOM
- **Image distillation** — produces a minimal rebuilt image keeping only what the workload needs
- **Wasm migration** — identifies code that can be compiled to Wasm; generates a migration plan
- **LLM orchestration** — `AiOrchestrator` drives analysis via Ollama, Claude, or OpenAI with shared prompt caching (SHA-256 keyed, configurable TTL) and retry with exponential backoff
- **CVE scanning** — integrates SBOM data with vulnerability feeds

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
tpt-chisel-core = "0.1"
```

Configure via `chisel.yaml`:

```yaml
ai:
  backend: local   # "local" → Ollama, "cloud" → Claude/OpenAI
  model: llama3
```

```rust
use tpt_chisel_core::{ChiselConfig, Analyzer};

let config = ChiselConfig::from_file("chisel.yaml").await?;
let analyzer = Analyzer::new(config).await?;
let report = analyzer.analyze_image("/path/to/image").await?;
```

## Part of TPT Boxcar

See the [monorepo README](../../README.md) for the full product suite.

## License

Apache-2.0
