# Contributing to TPT Cloud-Native

Thank you for your interest in contributing! This document covers how to set up your development environment and the conventions for branches, commits, and pull requests.

## Prerequisites

- **Rust** (stable toolchain) — `rustup update stable`
- **Go** 1.22+
- **Node.js** 18+
- **Docker** (for containerd-based Origin features)

## Development Setup

```bash
# Clone the repository
git clone https://github.com/tpt-solutions/tpt-cloud-native.git
cd tpt-cloud-native

# Build Rust workspace
cargo build

# Build Go components
cd tether/control-plane && go build ./... && cd ../..
cd chisel/core && go build ./... && cd ../..

# Install frontend dependencies
cd scope/dashboard && npm install && cd ../..
cd origin/gui && npm install && cd ../..
```

## Branch Strategy

- `main` — stable release branch; always deployable
- `develop` — integration branch for the next release
- `feature/<product>-<short-desc>` — new features (e.g. `feature/origin-ebpf-networking`)
- `fix/<product>-<short-desc>` — bug fixes
- `docs/<topic>` — documentation-only changes
- `chore/<topic>` — CI, tooling, and maintenance

## Commit Conventions

We follow [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <description>

[optional body]

[optional footer(s)]
```

**Types:** `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`

**Scopes** (product area): `origin`, `tether`, `scope`, `chisel`, `frontier`, `workspace`, `docs`

**Examples:**
```
feat(origin): add DNS resolver for .local domains
fix(tether): handle connection pool exhaustion gracefully
docs(chisel): add Wasm migration troubleshooting guide
```

## Pull Request Process

1. Create a feature/fix branch from `develop`
2. Write tests for any new functionality
3. Ensure all CI checks pass (`cargo test`, `cargo clippy`, `go test`, `golangci-lint`)
4. Request review from at least one maintainer
5. Squash-merge into `develop`; delete the feature branch after merge

## Code Style

- **Rust:** `rustfmt` default settings, `clippy` warnings are errors
- **Go:** `golangci-lint` default config (see `.golangci.yml`)
- Keep PRs focused — one logical change per PR
- Write meaningful commit messages that explain *why*, not just *what*

## License

By contributing, you agree that your contributions will be licensed under the Apache License 2.0.
