# Contributing to TPT Cloud-Native

Thank you for your interest in contributing! This document covers how to set up your development environment and the conventions for branches, commits, and pull requests.

## Development Environment Setup

### Prerequisites

- **Rust** (stable toolchain) — `rustup update stable`
  - Add the `wasm32-wasi` target: `rustup target add wasm32-wasi`
- **Go** 1.22+
- **Node.js** 20+
- **protoc** (Protocol Buffers compiler) — required for Frontier's gRPC types
  - macOS: `brew install protobuf`
  - Ubuntu/Debian: `apt install protobuf-compiler`
  - Windows: download from [github.com/protocolbuffers/protobuf/releases](https://github.com/protocolbuffers/protobuf/releases)
- **Docker** (for containerd-based Origin features)

### Cloning and Workspace Setup

```bash
# Clone the repository
git clone https://github.com/tpt-solutions/tpt-cloud-native.git
cd tpt-cloud-native

# Build the entire Rust workspace (all seven crates)
cargo build --workspace

# Build Go components (go.work covers all three modules)
go build ./tether/control-plane/...
go build ./scope/backend/...
go build ./frontier/control-plane/...

# Install frontend dependencies
cd scope/dashboard && npm install && cd ../..
cd origin/gui && npm install && cd ../..
```

### Code Style Tools

**Rust** — format with `rustfmt` (enforced in CI):

```bash
cargo fmt --all           # reformat all crates
cargo fmt --all -- --check  # CI check (fails on diff)
cargo clippy --workspace -- -D warnings  # lint; warnings are errors
```

**Go** — format with `gofmt` and lint with `golangci-lint`:

```bash
gofmt -w .               # reformat all Go source in the current module
golangci-lint run ./...  # run from any Go module dir or repo root
```

**TypeScript** — format and lint with Prettier/ESLint via npm scripts:

```bash
cd scope/dashboard && npm run lint   # Scope React dashboard
cd origin/gui && npm run lint        # Origin Tauri GUI
```

### PR Conventions

**Branch naming** — use one of the following prefixes:

| Prefix | Purpose |
|--------|---------|
| `feat/` | New feature (e.g. `feat/origin-ebpf-networking`) |
| `fix/` | Bug fix (e.g. `fix/tether-pool-exhaustion`) |
| `chore/` | CI, tooling, dependency, or maintenance work |
| `docs/` | Documentation-only changes |
| `refactor/` | Code restructuring with no behaviour change |

**Commit message format** — we follow [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <description>

[optional body — explain *why*, not just *what*]

[optional footer(s) — e.g. Closes #42, BREAKING CHANGE: ...]
```

**Types:** `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`

**Scopes** (product area): `origin`, `tether`, `scope`, `chisel`, `frontier`, `workspace`, `docs`

**Examples:**

```
feat(origin): add DNS resolver for .local domains
fix(tether): handle connection pool exhaustion gracefully
docs(chisel): add Wasm migration troubleshooting guide
chore(workspace): upgrade tokio to 1.38
```

**PR description** — use the `.github/PULL_REQUEST_TEMPLATE.md` provided in the repo. At a minimum include:

- A short summary of what the PR does and why.
- The issue(s) it closes (`Closes #<number>`).
- The type of change (bug fix / new feature / breaking change / docs / refactor).
- Confirmation that `cargo fmt`, `cargo clippy`, `gofmt`, and `golangci-lint` all pass and that tests have been added or updated.

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
