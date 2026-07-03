package tools

import (
	"context"
	"net/url"

	"github.com/modelcontextprotocol/go-sdk/mcp"
	"github.com/tpt-boxcar/mcp-server/internal/auth"
)

type ScopeQueryArgs struct {
	Since   string `json:"since,omitempty" jsonschema:"time window as a Go duration, e.g. '15m', '1h' (default: backend's default, currently 1h)"`
	Service string `json:"service,omitempty" jsonschema:"filter to a single service name"`
}

func (a ScopeQueryArgs) query() string {
	q := url.Values{}
	if a.Since != "" {
		q.Set("since", a.Since)
	}
	if a.Service != "" {
		q.Set("service", a.Service)
	}
	if len(q) == 0 {
		return ""
	}
	return "?" + q.Encode()
}

func ScopeQueryTraces(ctx context.Context, req *mcp.CallToolRequest, args ScopeQueryArgs) (*mcp.CallToolResult, any, error) {
	resp, err := doJSON("GET", scopeAddr()+"/api/v1/traces"+args.query(), auth.ScopeKey(), nil)
	if err != nil {
		return nil, nil, err
	}
	return textResult(resp), nil, nil
}

func ScopeQueryMetrics(ctx context.Context, req *mcp.CallToolRequest, args ScopeQueryArgs) (*mcp.CallToolResult, any, error) {
	resp, err := doJSON("GET", scopeAddr()+"/api/v1/metrics"+args.query(), auth.ScopeKey(), nil)
	if err != nil {
		return nil, nil, err
	}
	return textResult(resp), nil, nil
}

func ScopeQueryLogs(ctx context.Context, req *mcp.CallToolRequest, args ScopeQueryArgs) (*mcp.CallToolResult, any, error) {
	resp, err := doJSON("GET", scopeAddr()+"/api/v1/logs"+args.query(), auth.ScopeKey(), nil)
	if err != nil {
		return nil, nil, err
	}
	return textResult(resp), nil, nil
}
