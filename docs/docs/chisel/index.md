---
sidebar_position: 1
title: TPT Chisel
---

# TPT Chisel — Automated Distiller & Migrator

The "Bloat Sculptor" — analyzes runtime behavior and chips away unused OS layers, creating minimal distroless images.

## How It Works

### Phase 1: Trace & Distill

```bash
# Analyze a container
chisel analyze --image ubuntu:latest --entrypoint "./myapp"

# Distill to minimal image
chisel distill --image ubuntu:latest --output myapp-slim
```

### Phase 2: Wasm Migration

```bash
# Detect language and compile to Wasm
chisel wasm-migrate --image myapp:latest --output myapp.wasm
```

## CI/CD Integration

### GitHub Actions

```yaml
- uses: tpt-cloud-native/chisel@v1
  with:
    image: myapp:latest
    scan-cve: "true"
```

### GitLab CI

```yaml
include:
  - component: tpt-cloud-native/chisel/distill@v1

chisel-distill:
  variables:
    IMAGE: myapp:latest
```

## Output

- Distilled image (distroless/scratch base)
- SBOM (CycloneDX or SPDX 2.3)
- CVE scan report
- Wasm module (if migration applicable)
