# TPT Boxcar MCP Server

Exposes Origin, Tether, Scope, Chisel, and Frontier as MCP tools over stdio, so an AI agent (Claude Code, Claude Desktop, etc.) can drive them directly instead of a human running curl/CLI commands by hand.

## Build

```bash
cd mcp-server && go build -o mcp-server .
```

## Configure

Point an MCP client at the built binary (stdio transport, no extra args needed). For Claude Code / Claude Desktop, add it as a local MCP server pointing `command` at the built binary's path.

Environment variables (all optional — sensible localhost defaults are used):

| Variable | Default | Purpose |
|---|---|---|
| `TETHER_ADDR` | `http://localhost:8080` | Tether control plane REST base URL |
| `SCOPE_ADDR` | `http://localhost:8081` | Scope query API base URL |
| `FRONTIER_ADDR` | `http://localhost:8090` | Frontier control plane REST base URL |
| `TETHER_API_KEY` / `SCOPE_API_KEY` / `FRONTIER_API_KEY` | unset | Forwarded as `X-API-Key` to the matching product, same variables those control planes already read |
| `TPT_ORIGIN_BIN` | `tpt` | Path to the Origin CLI binary (must be on `$PATH` or set explicitly) |
| `TPT_CHISEL_BIN` | `chisel` | Path to the Chisel CLI binary |

## Tools

| Tool | Wraps |
|---|---|
| `origin_sandbox_up` / `_down` / `_status` | `tpt origin up/down/ps` (spawns `up` in the background, since it blocks in the foreground until Ctrl+C) |
| `tether_configure_route` / `tether_list_backends` | Tether control plane REST (`/api/v1/backends`, `/api/v1/routes`) |
| `scope_query_traces` / `_metrics` / `_logs` | Scope query API (`/api/v1/traces`, `/metrics`, `/logs`, with `since`/`service` filters) |
| `chisel_analyze_image` / `chisel_distill_image` | `chisel analyze/distill --json` |
| `frontier_create_route` / `frontier_list_routes` / `frontier_create_upstream` | Frontier control plane REST (`/api/v1/routes`, `/api/v1/upstreams`) |

REST-backed tools are plain `net/http` clients (`internal/tools/httpclient.go`) — no new protocol logic, just JSON pass-through. CLI-backed tools (`origin_*`, `chisel_*`) shell out via `os/exec` (`internal/tools/exec.go`); both CLIs send their tracing/log output to stderr so stdout stays clean JSON for the `--json` variants.

## Current limitations

- `origin_sandbox_up` starts the process detached but the MCP server does not track it beyond the child's PID for reaping — the actual "is it still up" bookkeeping is the CLI's own state file (`origin/cli/src/state.rs`), which `origin_sandbox_down`/`_status` re-query by running `tpt origin down`/`ps` in the same directory as the manifest.
- `frontier_create_route`/`frontier_create_upstream` and `tether_configure_route` use REST, not gRPC — Frontier's gRPC surface additionally offers `WatchConfig` streaming, which no tool here uses yet.
- No tool wraps Chisel's AI-backed `migrate`/`audit` subcommands yet (only the non-AI `analyze`/`distill`).
