package tools

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"time"

	"github.com/modelcontextprotocol/go-sdk/mcp"
)

const originOpTimeout = 15 * time.Second

type OriginManifestArgs struct {
	Manifest string `json:"manifest,omitempty" jsonschema:"path to the manifest.yaml (default: manifest.yaml in the current directory)"`
}

func (a OriginManifestArgs) resolve() (manifest, dir string) {
	manifest = a.Manifest
	if manifest == "" {
		manifest = "manifest.yaml"
	}
	dir = filepath.Dir(manifest)
	if dir == "" {
		dir = "."
	}
	return manifest, dir
}

// OriginSandboxUp starts `tpt origin up` as a detached background process.
// `up` blocks in the foreground until Ctrl+C, so this tool does not wait for
// it to exit — it starts the process and returns immediately. State is
// tracked the same way the CLI already tracks it for a second terminal
// (origin/cli/src/state.rs's state file, written into the manifest's
// directory), so origin_sandbox_down/status below just re-run the CLI's own
// `down`/`ps` against that same directory rather than duplicating process
// bookkeeping here.
func OriginSandboxUp(ctx context.Context, req *mcp.CallToolRequest, args OriginManifestArgs) (*mcp.CallToolResult, any, error) {
	manifest, dir := args.resolve()

	cmd := exec.Command(tptBin(), "origin", "up", "--manifest", manifest)
	cmd.Dir = dir
	cmd.Stdout = os.Stderr
	cmd.Stderr = os.Stderr

	if err := cmd.Start(); err != nil {
		return nil, nil, fmt.Errorf("failed to start tpt origin up: %w", err)
	}
	pid := cmd.Process.Pid
	go cmd.Wait() // reap in the background; this tool doesn't block on it

	return textResult(fmt.Sprintf(
		"Started `tpt origin up --manifest %s` in %s (pid %d). It will keep running in the background — use origin_sandbox_status or origin_sandbox_down to manage it.",
		manifest, dir, pid,
	)), nil, nil
}

func OriginSandboxDown(ctx context.Context, req *mcp.CallToolRequest, args OriginManifestArgs) (*mcp.CallToolResult, any, error) {
	_, dir := args.resolve()
	out, err := runInDir(ctx, originOpTimeout, dir, tptBin(), "origin", "down")
	if err != nil {
		return nil, nil, err
	}
	return textResult(out), nil, nil
}

func OriginSandboxStatus(ctx context.Context, req *mcp.CallToolRequest, args OriginManifestArgs) (*mcp.CallToolResult, any, error) {
	_, dir := args.resolve()
	out, err := runInDir(ctx, originOpTimeout, dir, tptBin(), "origin", "ps")
	if err != nil {
		return nil, nil, err
	}
	return textResult(out), nil, nil
}
