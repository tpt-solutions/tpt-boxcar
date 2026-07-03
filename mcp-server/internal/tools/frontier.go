package tools

import (
	"context"
	"encoding/json"
	"fmt"

	"github.com/modelcontextprotocol/go-sdk/mcp"
	"github.com/tpt-boxcar/mcp-server/internal/auth"
)

type FrontierCreateRouteArgs struct {
	Name        string `json:"name" jsonschema:"unique route name"`
	Prefix      string `json:"prefix" jsonschema:"path prefix to match, e.g. '/api'"`
	Cluster     string `json:"cluster" jsonschema:"name of the upstream to route to"`
	Priority    int    `json:"priority,omitempty" jsonschema:"route priority (higher wins)"`
	StripPrefix bool   `json:"strip_prefix,omitempty" jsonschema:"strip the matched prefix before forwarding"`
}

func FrontierCreateRoute(ctx context.Context, req *mcp.CallToolRequest, args FrontierCreateRouteArgs) (*mcp.CallToolResult, any, error) {
	body, err := json.Marshal(args)
	if err != nil {
		return nil, nil, fmt.Errorf("marshal route: %w", err)
	}
	resp, err := doJSON("POST", frontierAddr()+"/api/v1/routes", auth.FrontierKey(), body)
	if err != nil {
		return nil, nil, err
	}
	return textResult(resp), nil, nil
}

type FrontierListRoutesArgs struct{}

func FrontierListRoutes(ctx context.Context, req *mcp.CallToolRequest, args FrontierListRoutesArgs) (*mcp.CallToolResult, any, error) {
	resp, err := doJSON("GET", frontierAddr()+"/api/v1/routes", auth.FrontierKey(), nil)
	if err != nil {
		return nil, nil, err
	}
	return textResult(resp), nil, nil
}

type FrontierCreateUpstreamArgs struct {
	Name           string   `json:"name" jsonschema:"unique upstream name"`
	Type           string   `json:"type,omitempty" jsonschema:"upstream type, e.g. 'http'"`
	Endpoints      []string `json:"endpoints" jsonschema:"backend endpoints, e.g. ['127.0.0.1:9000']"`
	ConnectTimeout string   `json:"connect_timeout,omitempty" jsonschema:"connect timeout, e.g. '5s'"`
	LbPolicy       string   `json:"lb_policy,omitempty" jsonschema:"load-balancing policy, e.g. 'round_robin'"`
}

func FrontierCreateUpstream(ctx context.Context, req *mcp.CallToolRequest, args FrontierCreateUpstreamArgs) (*mcp.CallToolResult, any, error) {
	body, err := json.Marshal(args)
	if err != nil {
		return nil, nil, fmt.Errorf("marshal upstream: %w", err)
	}
	resp, err := doJSON("POST", frontierAddr()+"/api/v1/upstreams", auth.FrontierKey(), body)
	if err != nil {
		return nil, nil, err
	}
	return textResult(resp), nil, nil
}
