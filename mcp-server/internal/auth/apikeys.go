// Package auth reads the same API key environment variables the Go control
// planes themselves already check (TETHER_API_KEY, SCOPE_API_KEY,
// FRONTIER_API_KEY), so the MCP server authenticates with existing
// infrastructure rather than inventing a new mechanism.
package auth

import "os"

func TetherKey() string   { return os.Getenv("TETHER_API_KEY") }
func ScopeKey() string    { return os.Getenv("SCOPE_API_KEY") }
func FrontierKey() string { return os.Getenv("FRONTIER_API_KEY") }
