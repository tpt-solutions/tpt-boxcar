---
sidebar_position: 2
title: Local dev sandbox without a Docker daemon
---

# Local dev sandbox without a Docker daemon

**Problem:** you want a Postgres container and a small API service running
side by side for local development, without installing Docker Desktop or
running a background daemon.

**How:** Origin reads one manifest and spins up OCI containers and Wasm
modules from the same file, using containerd + Wasmtime directly — no daemon,
no VM.

```bash
cargo build -p tpt-origin
tpt origin init --dir demo   # scaffolds demo/manifest.yaml
cd demo
tpt origin up
```

The scaffolded manifest (`origin/examples/getting-started/manifest.yaml`,
copied in by `init`) defines two services:

```yaml
services:
  db:
    type: oci
    image: postgres:16
    ports:
      - host: 5432
        container: 5432
    environment:
      POSTGRES_PASSWORD: CHANGE_ME
    volumes:
      - source: pgdata
        target: /var/lib/postgresql/data

  api:
    type: wasm
    path: ./target/api.wasm
    environment:
      DB_HOST: db
      DB_PORT: "5432"
    depends_on:
      - db
```

`api` resolves `db` via Origin's local DNS (`db.local`) without any manual
network configuration.

**What you should see:** `tpt origin ps` lists both services running;
`tpt origin logs api` streams the Wasm service's output; `tpt origin down`
tears everything down cleanly.

**Where to go next:** [origin/README.md](https://github.com/tpt-boxcar/tpt-boxcar/blob/main/origin/README.md)
for the full manifest format, `--watch` (restart on source change), and
`--rootless` (no root privileges required).
