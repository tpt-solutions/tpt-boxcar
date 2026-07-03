# TPT Origin

Unified local sandbox: run OCI containers and Wasm modules side by side from a single manifest, with zero-config service discovery.

## Quickstart (5 minutes)

```bash
cargo build -p tpt-origin
# binary is target/debug/tpt

cp -r origin/examples/getting-started/manifest.yaml .
./target/debug/tpt origin up
```

In a second terminal, from the same directory:

```bash
./target/debug/tpt origin ps     # lists services from the running environment
./target/debug/tpt origin down   # tears it down
```

Or scaffold a fresh manifest instead of using the bundled example:

```bash
./target/debug/tpt origin init --dir my-app && cd my-app
```

## Manifest format

See [`examples/getting-started/manifest.yaml`](examples/getting-started/manifest.yaml) for a full example (an OCI Postgres container plus a dependent Wasm service). Three service types are supported:

- `type: oci` — an OCI container image. **Bookkeeping only today**: the manifest is parsed and tracked, but no real containerd integration exists yet, so nothing is actually spawned.
- `type: wasm` — a Wasm module. Same limitation as `oci` — tracked, not yet actually instantiated.
- `type: process` — a native command (`command: ["go", "run", "./some/binary"]`, plus optional `environment`/`working_dir`). This one is real: Origin spawns an actual child process, tracks its PID, and kills it on `tpt origin down` (see [`examples/demo-stack/manifest.yaml`](../examples/demo-stack/manifest.yaml) at the repo root for a working example that brings up Scope + Frontier's control planes this way).

## Current limitations

- `tpt origin logs` and `tpt origin exec` are not yet implemented (they print a placeholder message).
- `tpt origin up` runs in the foreground until Ctrl+C (or until a `tpt origin down` from another terminal signals it) — there is no background/daemon mode yet.
- `type: oci` and `type: wasm` services don't actually spawn anything (see above) — only `type: process` does today. `up`/`ps`/`down` reflect real process state for `type: process` services; for the other two, they reflect manifest/lifecycle bookkeeping only.
- `depends_on` is accepted in the manifest but not yet used to order startup — services start in map-iteration order (unspecified).
