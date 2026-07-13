# tpt-chisel

[![Crates.io](https://img.shields.io/crates/v/tpt-chisel.svg)](https://crates.io/crates/tpt-chisel)
[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](../../LICENSE)

CLI for **TPT Chisel** — automatically distill OCI images and migrate workloads to Wasm with LLM guidance.

## Install

```bash
cargo install tpt-chisel
```

## Quick start

```bash
# Analyse an image directory and generate an SBOM + migration plan
chisel analyze ./my-image

# Distill the image: rebuild it keeping only runtime-required layers
chisel distill ./my-image --dockerfile

# Generate a Wasm migration plan with LLM analysis
chisel migrate ./my-image

# Run a CVE security audit against the SBOM
chisel audit ./my-image
```

## Commands

| Command | Description |
|---------|-------------|
| `chisel analyze <dir>` | Scan layers, build dependency graph, generate SBOM |
| `chisel distill <dir>` | Produce a minimal rebuilt image |
| `chisel migrate <dir>` | LLM-assisted Wasm migration plan |
| `chisel audit <dir>` | CVE scan using the generated SBOM |

## Configuration

Chisel reads `chisel.yaml` from the working directory:

```yaml
ai:
  backend: local   # "local" → Ollama, "cloud" → Claude/OpenAI
  model: llama3
  cache_ttl_secs: 3600
```

## Part of TPT Boxcar

See the [monorepo README](../../README.md) for the full product suite.

## License

Apache-2.0
