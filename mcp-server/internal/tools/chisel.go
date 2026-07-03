package tools

import (
	"context"
	"time"

	"github.com/modelcontextprotocol/go-sdk/mcp"
)

const chiselTimeout = 60 * time.Second

type ChiselImageArgs struct {
	Image string `json:"image" jsonschema:"path to an extracted OCI image directory"`
}

func ChiselAnalyzeImage(ctx context.Context, req *mcp.CallToolRequest, args ChiselImageArgs) (*mcp.CallToolResult, any, error) {
	out, err := runCapture(ctx, chiselTimeout, chiselBin(), "analyze", args.Image, "--json")
	if err != nil {
		return nil, nil, err
	}
	return textResult(out), nil, nil
}

func ChiselDistillImage(ctx context.Context, req *mcp.CallToolRequest, args ChiselImageArgs) (*mcp.CallToolResult, any, error) {
	out, err := runCapture(ctx, chiselTimeout, chiselBin(), "distill", args.Image, "--json")
	if err != nil {
		return nil, nil, err
	}
	return textResult(out), nil, nil
}
