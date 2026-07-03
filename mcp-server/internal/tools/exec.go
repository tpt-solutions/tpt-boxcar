package tools

import (
	"bytes"
	"context"
	"fmt"
	"os/exec"
	"time"
)

// runCapture runs a CLI command and returns its stdout as text. Both CLIs
// this package shells out to (tpt, chisel) send tracing/log output to
// stderr, so stdout is safe to treat as the tool's machine-readable result.
func runCapture(ctx context.Context, timeout time.Duration, name string, args ...string) (string, error) {
	ctx, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()

	cmd := exec.CommandContext(ctx, name, args...)
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	if err := cmd.Run(); err != nil {
		return "", fmt.Errorf("%s %v failed: %w\n%s", name, args, err, stderr.String())
	}
	return stdout.String(), nil
}

// runInDir is like runCapture but runs the command in a specific working
// directory (used by the Origin tools, which key their state file off cwd).
func runInDir(ctx context.Context, timeout time.Duration, dir, name string, args ...string) (string, error) {
	ctx, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()

	cmd := exec.CommandContext(ctx, name, args...)
	cmd.Dir = dir
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	if err := cmd.Run(); err != nil {
		return "", fmt.Errorf("%s %v (in %s) failed: %w\n%s", name, args, dir, err, stderr.String())
	}
	return stdout.String(), nil
}
