package tools

import (
	"context"
	"encoding/json"
	"fmt"

	"github.com/modelcontextprotocol/go-sdk/mcp"
	"github.com/tpt-boxcar/mcp-server/internal/auth"
)

type TetherConfigureRouteArgs struct {
	BackendID string         `json:"backend_id" jsonschema:"ID of an existing backend to route to"`
	Backend   map[string]any `json:"backend,omitempty" jsonschema:"optional backend to create first (id, type, host, port, database, options)"`
	RouteID   string         `json:"route_id" jsonschema:"unique ID for the new route"`
	Pattern   string         `json:"pattern" jsonschema:"route match pattern, e.g. 'app/*'"`
	Priority  int            `json:"priority,omitempty" jsonschema:"route priority (higher wins)"`
}

func TetherConfigureRoute(ctx context.Context, req *mcp.CallToolRequest, args TetherConfigureRouteArgs) (*mcp.CallToolResult, any, error) {
	key := auth.TetherKey()
	base := tetherAddr()

	var results []string

	if args.Backend != nil {
		body, err := json.Marshal(args.Backend)
		if err != nil {
			return nil, nil, fmt.Errorf("marshal backend: %w", err)
		}
		resp, err := doJSON("POST", base+"/api/v1/backends", key, body)
		if err != nil {
			return nil, nil, fmt.Errorf("create backend: %w", err)
		}
		results = append(results, "backend created: "+resp)
	}

	route := map[string]any{
		"id":         args.RouteID,
		"pattern":    args.Pattern,
		"backend_id": args.BackendID,
		"priority":   args.Priority,
	}
	body, err := json.Marshal(route)
	if err != nil {
		return nil, nil, fmt.Errorf("marshal route: %w", err)
	}
	resp, err := doJSON("POST", base+"/api/v1/routes", key, body)
	if err != nil {
		return nil, nil, fmt.Errorf("create route: %w", err)
	}
	results = append(results, "route created: "+resp)

	return textResult(joinLines(results)), nil, nil
}

type TetherListBackendsArgs struct{}

func TetherListBackends(ctx context.Context, req *mcp.CallToolRequest, args TetherListBackendsArgs) (*mcp.CallToolResult, any, error) {
	resp, err := doJSON("GET", tetherAddr()+"/api/v1/backends", auth.TetherKey(), nil)
	if err != nil {
		return nil, nil, err
	}
	return textResult(resp), nil, nil
}
