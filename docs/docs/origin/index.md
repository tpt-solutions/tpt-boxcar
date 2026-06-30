---
sidebar_position: 1
title: TPT Origin
slug: /
---

# TPT Origin — Unified Local Sandbox

The "Anti-Docker Desktop" — seamlessly orchestrates OCI containers and Wasm modules side-by-side.

## Quick Start

```bash
# Install
cargo install tpt-origin

# Scaffold a manifest
tpt origin init

# Start services
tpt origin up

# List running services
tpt origin ps

# View logs
tpt origin logs api

# Stop everything
tpt origin down
```

## Manifest Format

```yaml
name: my-app
version: "1.0"

services:
  db:
    type: oci
    image: postgres:16
    ports:
      - host: 5432
        container: 5432
    environment:
      POSTGRES_PASSWORD: changeme

  api:
    type: wasm
    path: ./target/api.wasm
    depends_on:
      - db
```

## Features

- **Daemonless** — native binary, no background VM
- **Mixed runtimes** — OCI containers + Wasm modules from one manifest
- **Auto networking** — eBPF-powered service mesh with zero config
- **Local DNS** — `<service>.local` resolves automatically
- **Desktop GUI** — Tauri-based visual management
