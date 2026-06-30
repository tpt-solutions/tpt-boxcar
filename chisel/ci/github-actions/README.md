# TPT Chisel GitHub Actions Integration

## Usage

### Basic — Distill a Docker image

```yaml
- uses: tpt-cloud-native/chisel@v1
  with:
    image: myapp:latest
```

### Full — Distill + CVE scan + SBOM

```yaml
- uses: tpt-cloud-native/chisel@v1
  id: chisel
  with:
    image: myapp:latest
    output: myapp-slim
    scan-cve: "true"
    generate-sbom: "true"
    sbom-format: "spdx"

- name: Use distilled image
  run: |
    echo "Distilled image: ${{ steps.chisel.outputs.distilled-image }}"
    echo "SBOM: ${{ steps.chisel.outputs.sbom-path }}"
    echo "CVE Report: ${{ steps.chisel.outputs.cve-report }}"
```

### Wasm Migration

```yaml
- uses: tpt-cloud-native/chisel@v1
  with:
    image: myapp:latest
    wasm-migrate: "true"
```

## Outputs

| Output | Description |
|--------|-------------|
| `distilled-image` | Name of the distilled image |
| `sbom-path` | Path to generated SBOM file |
| `cve-report` | Path to CVE scan report |
| `wasm-module` | Path to compiled Wasm module (if migration enabled) |

## Example: Full CI Pipeline

```yaml
name: Build & Distill
on: [push]

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Build image
        run: docker build -t myapp:${{ github.sha }} .

      - name: Distill image
        uses: tpt-cloud-native/chisel@v1
        id: chisel
        with:
          image: myapp:${{ github.sha }}

      - name: Push slim image
        run: |
          docker tag ${{ steps.chisel.outputs.distilled-image }} registry/myapp:slim
          docker push registry/myapp:slim
```
