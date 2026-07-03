package tools

import "os"

func envOr(key, fallback string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return fallback
}

func tetherAddr() string   { return envOr("TETHER_ADDR", "http://localhost:8080") }
func scopeAddr() string    { return envOr("SCOPE_ADDR", "http://localhost:8081") }
func frontierAddr() string { return envOr("FRONTIER_ADDR", "http://localhost:8090") }

// tptBin/chiselBin let the MCP server locate the CLI binaries it shells out
// to without hardcoding a path — default to relying on $PATH.
func tptBin() string    { return envOr("TPT_ORIGIN_BIN", "tpt") }
func chiselBin() string { return envOr("TPT_CHISEL_BIN", "chisel") }
