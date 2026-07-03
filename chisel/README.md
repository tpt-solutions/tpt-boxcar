# TPT Chisel

Automated image distiller & Wasm migrator: analyzes a container image's runtime footprint, produces a minimal distilled image (with SBOM + CVE scan), and evaluates Wasm migration feasibility.

## Quickstart (5 minutes)

```bash
cargo build -p tpt-chisel
# binary is target/debug/chisel

./target/debug/chisel analyze path/to/extracted-image
./target/debug/chisel distill path/to/extracted-image --dockerfile
```

`analyze` expects a directory containing an (optional) `manifest.json`, `config.json`, `trace.json`, and `deps.json` describing the image — see `chisel/core/src/analyzer.rs` for the exact shape each file is read as. If these files are absent, Chisel falls back to a synthetic/empty analysis rather than failing, which is useful for trying the CLI without a real image on hand.

`distill` runs the same analysis, then produces a `DistilledImage` with an embedded CycloneDX SBOM, a CVE scan, and (when the image is Wasm-compatible) a suggested migration path. Add `--sbom spdx` to also print an SPDX-format SBOM, or `--dockerfile` to print a generated Dockerfile for the distilled result. Add `--json` to either command for machine-readable output.

## AI-assisted analysis

`chisel/core/src/ai/` contains a fully built LLM orchestration layer (`AiOrchestrator`, supporting Ollama, Claude, and OpenAI as backends). The CLI (`chisel/cli/src/config.rs`) additionally supports **OpenRouter** as a cloud provider — it's OpenAI-compatible, so it reuses `OpenAiProvider` with `base_url: https://openrouter.ai/api` and OpenRouter's own namespaced model IDs (e.g. `anthropic/claude-3.5-sonnet`, `openai/gpt-4o`). Two CLI subcommands drive all of these:

```bash
cp chisel/examples/chisel.yaml .   # edit ai.backend / model / endpoint as needed

# local backend: requires Ollama running at ai.local.endpoint (default http://localhost:11434)
./target/debug/chisel migrate path/to/extracted-image --config chisel.yaml

# cloud backend: requires the env var named by ai.cloud.api_key_env (e.g. ANTHROPIC_API_KEY)
./target/debug/chisel audit path/to/extracted-image --config chisel.yaml --ai cloud
```

`migrate` analyzes the image and asks the LLM for a phased Wasm migration plan (`generate_migration_plan`). `audit` distills the image (for its SBOM + CVE scan) and asks the LLM for a security audit (`generate_security_audit`). Both accept `--ai local|cloud` to override the config file's `ai.backend` for a single run.

## Current limitations

- `analyze`/`distill` work against a pre-extracted image directory, not an image reference (`docker://`-style pulling is not implemented).
- `migrate`/`audit` expect the LLM to return well-formed JSON matching the `MigrationPlan`/`SecurityAudit` schema (`chisel/core/src/ai/prompt_library.rs` builds the prompt asking for this) — a model that doesn't follow the schema will fail to parse rather than degrade gracefully.
