// Command mcp-server exposes TPT Boxcar's five products (Origin, Tether,
// Scope, Chisel, Frontier) as MCP tools over stdio, so an AI agent can drive
// them directly instead of the human curl/CLI workflows documented in each
// product's README.
package main

import (
	"context"
	"log"

	"github.com/modelcontextprotocol/go-sdk/mcp"
	"github.com/tpt-boxcar/mcp-server/internal/tools"
)

func main() {
	server := mcp.NewServer(&mcp.Implementation{Name: "tpt-boxcar", Version: "0.1.0"}, nil)

	mcp.AddTool(server, &mcp.Tool{
		Name:        "origin_sandbox_up",
		Description: "Start a TPT Origin local sandbox from a manifest.yaml (spawns `tpt origin up` in the background).",
	}, tools.OriginSandboxUp)
	mcp.AddTool(server, &mcp.Tool{
		Name:        "origin_sandbox_down",
		Description: "Tear down the TPT Origin sandbox previously started with origin_sandbox_up.",
	}, tools.OriginSandboxDown)
	mcp.AddTool(server, &mcp.Tool{
		Name:        "origin_sandbox_status",
		Description: "List the services in the running TPT Origin sandbox.",
	}, tools.OriginSandboxStatus)

	mcp.AddTool(server, &mcp.Tool{
		Name:        "tether_configure_route",
		Description: "Register a Tether route (and optionally a backend) so a Wasm caller can reach a database through the connection proxy.",
	}, tools.TetherConfigureRoute)
	mcp.AddTool(server, &mcp.Tool{
		Name:        "tether_list_backends",
		Description: "List the database backends currently registered with Tether's control plane.",
	}, tools.TetherListBackends)

	mcp.AddTool(server, &mcp.Tool{
		Name:        "scope_query_traces",
		Description: "Query recent distributed traces ingested by TPT Scope, optionally filtered by service and time window.",
	}, tools.ScopeQueryTraces)
	mcp.AddTool(server, &mcp.Tool{
		Name:        "scope_query_metrics",
		Description: "Query recent metrics ingested by TPT Scope, optionally filtered by service and time window.",
	}, tools.ScopeQueryMetrics)
	mcp.AddTool(server, &mcp.Tool{
		Name:        "scope_query_logs",
		Description: "Query recent logs ingested by TPT Scope, optionally filtered by service and time window.",
	}, tools.ScopeQueryLogs)

	mcp.AddTool(server, &mcp.Tool{
		Name:        "chisel_analyze_image",
		Description: "Run TPT Chisel's two-phase analysis (runtime trace + Wasm migration feasibility) on an extracted OCI image directory.",
	}, tools.ChiselAnalyzeImage)
	mcp.AddTool(server, &mcp.Tool{
		Name:        "chisel_distill_image",
		Description: "Distill an extracted OCI image into a minimal image with an SBOM and CVE scan via TPT Chisel.",
	}, tools.ChiselDistillImage)

	mcp.AddTool(server, &mcp.Tool{
		Name:        "frontier_create_route",
		Description: "Create a route on TPT Frontier's edge gateway, mapping a path prefix to an upstream.",
	}, tools.FrontierCreateRoute)
	mcp.AddTool(server, &mcp.Tool{
		Name:        "frontier_list_routes",
		Description: "List the routes currently configured on TPT Frontier's control plane.",
	}, tools.FrontierListRoutes)
	mcp.AddTool(server, &mcp.Tool{
		Name:        "frontier_create_upstream",
		Description: "Create an upstream (backend cluster) on TPT Frontier's control plane.",
	}, tools.FrontierCreateUpstream)

	if err := server.Run(context.Background(), &mcp.StdioTransport{}); err != nil {
		log.Fatalf("mcp-server: %v", err)
	}
}
