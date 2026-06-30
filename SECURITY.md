# Security Policy

## Vulnerability Reporting

If you discover a security vulnerability in TPT Cloud Native, please report it responsibly:

1. **Do not** open a public GitHub issue for security vulnerabilities.
2. Email security reports to: `security@tpt-cloud-native.dev` (or the maintainer's private email).
3. Include:
   - Description of the vulnerability
   - Steps to reproduce
   - Potential impact assessment
   - Suggested fix (if any)

You should receive an initial response within 72 hours. We will coordinate disclosure after a fix is available.

## Audit Tools

The following tools are used as part of our security audit process:

### Rust

```bash
# Dependency vulnerability audit
cargo audit

# Go vulnerability checker (for Go components)
govulncheck ./...

# Lint for common mistakes and unsafe patterns
cargo clippy --workspace -- -D warnings
```

### Supply Chain

- All dependencies are pinned in `Cargo.lock` and `go.sum`.
- Dependabot / Renovate PRs are reviewed before merge.
- SBOM generation via `cargo dist` and `chisel/distiller`.

## eBPF Probes Audit Checklist

The scope agent uses eBPF probes for runtime observability. These must be audited:

- [ ] Probe programs have correct BPF verifier pass (no unbounded loops)
- [ ] All map sizes are bounded and documented
- [ ] Probe attachment is restricted to approved hook points
- [ ] No arbitrary user-space memory reads beyond process boundaries
- [ ] Probe detach/cleanup on process exit is guaranteed
- [ ] Privilege escalation paths are documented (CAP_BPF, CAP_NET_ADMIN)
- [ ] Ring buffer overflow behavior is tested
- [ ] Probe programs are compiled with `-O2` and verified with `bpftool prog show`

## Wasm Sandboxing Audit Checklist

TPT runs user-provided WebAssembly modules. Sandboxing guarantees:

- [ ] All Wasm modules run inside a WASI-compatible runtime (wasmtime)
- [ ] No direct filesystem access — only declared preopened directories
- [ ] No network access unless explicitly granted via WASI sockets
- [ ] Memory limits are enforced (`wasmtime::Config::max_memory`)
- [ ] Fuel/epoch-based execution limits prevent infinite loops
- [ ] Module compilation is validated before instantiation
- [ ] Host function imports are allow-listed per plugin manifest
- [ ] Panic/unreachable traps are caught and logged without crashing the host
- [ ] Multi-instance isolation: each module gets its own `Store` and `Memory`
- [ ] Fuzzing harness runs against the module loader (`cargo fuzz run module_loader`)

## General Security Practices

- Secrets are never committed to the repository.
- TLS is enforced for all inter-service communication (tether proxy, frontier proxy).
- Database credentials are resolved from environment variables, not config files.
- All HTTP endpoints validate input and enforce rate limits where applicable.
- CI runs `cargo audit`, `govulncheck`, and `cargo clippy` on every PR.
