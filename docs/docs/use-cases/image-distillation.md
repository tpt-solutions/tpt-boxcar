---
sidebar_position: 5
title: Shrink an image and get a Wasm migration plan
---

# Shrink an image and get a Wasm migration plan

**Problem:** a container image has grown bloated over time, and you don't
know whether it's actually a good candidate for a Wasm rewrite, or just needs
a diet.

**How:** Chisel analyzes a pre-extracted image's runtime footprint, produces
a minimal distilled image with an SBOM and CVE scan, and — if you want an
LLM's opinion — a phased migration plan.

```bash
cargo build -p tpt-chisel
# binary is target/debug/chisel

./target/debug/chisel analyze path/to/extracted-image
./target/debug/chisel distill path/to/extracted-image --dockerfile
```

If `path/to/extracted-image` doesn't have a `manifest.json`/`config.json`/
`trace.json`/`deps.json`, Chisel falls back to a synthetic analysis rather
than failing — useful for a first try without a real image on hand.

For the AI-assisted migration plan, copy the bundled config
(`chisel/examples/chisel.yaml`) and pick a backend:

```bash
cp chisel/examples/chisel.yaml .   # edit ai.backend / model / endpoint as needed

# local: requires Ollama at ai.local.endpoint (default http://localhost:11434)
./target/debug/chisel migrate path/to/extracted-image --config chisel.yaml

# cloud: requires the env var named by ai.cloud.api_key_env, e.g. ANTHROPIC_API_KEY
./target/debug/chisel audit path/to/extracted-image --config chisel.yaml --ai cloud
```

**What you should see:** `distill` prints a `DistilledImage` with an embedded
CycloneDX SBOM and CVE scan (add `--sbom spdx` for SPDX, `--json` for
machine-readable output). `migrate` returns a phased Wasm migration plan;
`audit` returns a security audit — both parsed from the LLM's JSON response.

**Where to go next:** [chisel/README.md](https://github.com/tpt-boxcar/tpt-boxcar/blob/main/chisel/README.md)
for the OpenRouter provider option and current limitations (no `docker://`
pulling yet — you extract the image yourself first).
