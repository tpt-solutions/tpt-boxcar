# TPT Chisel GitLab CI Component

## Usage

### Basic — Distill a Docker image

```yaml
include:
  - component: tpt-cloud-native/chisel/distill@v1

chisel-distill:
  variables:
    IMAGE: myapp:latest
```

### Full — Distill + CVE scan + SBOM

```yaml
include:
  - component: tpt-cloud-native/chisel/distill@v1

chisel-distill:
  variables:
    IMAGE: myapp:latest
    OUTPUT: myapp-slim
    SCAN_CVE: "true"
    SBOM_FORMAT: "spdx"
```

### Use in a Pipeline

```yaml
stages:
  - build
  - distill
  - deploy

build-image:
  stage: build
  script:
    - docker build -t myapp:$CI_COMMIT_SHA .

distill-image:
  stage: distill
  include:
    - component: tpt-cloud-native/chisel/distill@v1
  variables:
    IMAGE: myapp:$CI_COMMIT_SHA

deploy:
  stage: deploy
  script:
    - docker push registry/myapp:slim
```

## Artifacts

The component produces:
- `sbom.cdx.json` or `sbom.spdx.json` — SBOM file
- `cve-report.json` — CVE scan results
