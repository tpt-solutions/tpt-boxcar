package main

import (
	"context"
	"crypto/subtle"
	"encoding/json"
	"fmt"
	"log"
	"net/http"
	"os"
	"strconv"
	"time"

	"github.com/tpt-boxcar/scope/backend/internal/ratelimit"

	"github.com/ClickHouse/clickhouse-go/v2"
	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
)

// --- Frontend API types (must match scope/dashboard/src/api/types.ts) ---

type TraceSpan struct {
	ID       string  `json:"id"`
	Name     string  `json:"name"`
	Service  string  `json:"service"`
	Start    float64 `json:"start"`    // ms since Unix epoch
	Duration float64 `json:"duration"` // ms
	ParentID *string `json:"parentId"`
}

type MetricPoint struct {
	Timestamp int64   `json:"timestamp"` // Unix ms
	Value     float64 `json:"value"`
}

type MetricSeries struct {
	Name   string        `json:"name"`
	Points []MetricPoint `json:"points"`
}

type LogEntry struct {
	Time     string `json:"time"`
	Service  string `json:"service"`
	Severity string `json:"severity"`
	Message  string `json:"message"`
}

type ServiceNode struct {
	Name         string   `json:"name"`
	Health       string   `json:"health"`
	Type         string   `json:"type"`
	Dependencies []string `json:"dependencies"`
}

type WasmModule struct {
	Name            string `json:"name"`
	CompileTime     string `json:"compileTime"`
	InstantiateTime string `json:"instantiateTime"`
	MemoryPages     int64  `json:"memoryPages"`
	MemoryMB        string `json:"memoryMB"`
}

// WasmInvocation is a captured Wasm module invocation's inputs, fetched by
// `origin replay` to re-run a module offline with identical args/env.
type WasmInvocation struct {
	Timestamp   string            `json:"timestamp"`
	ModuleName  string            `json:"moduleName"`
	Function    string            `json:"function"`
	Args        []string          `json:"args"`
	Env         map[string]string `json:"env"`
	WasmSha256  string            `json:"wasmSha256"`
	ServiceName string            `json:"serviceName"`
	ContainerID string            `json:"containerId"`
}

// --- Server ---

type QueryServer struct {
	db driver.Conn
}

func newQueryServer(ctx context.Context) (*QueryServer, error) {
	addr := env("CLICKHOUSE_ADDR", "127.0.0.1:9000")
	user := env("CLICKHOUSE_USER", "default")
	pass := os.Getenv("CLICKHOUSE_PASS")
	db := env("CLICKHOUSE_DB", "default")

	conn, err := clickhouse.Open(&clickhouse.Options{
		Addr: []string{addr},
		Auth: clickhouse.Auth{
			Database: db,
			Username: user,
			Password: pass,
		},
		DialTimeout:          5 * time.Second,
		MaxOpenConns:         5,
		MaxIdleConns:         2,
		ConnMaxLifetime:      10 * time.Minute,
		ConnOpenStrategy:     clickhouse.ConnOpenInOrder,
		BlockBufferSize:      10,
		MaxCompressionBuffer: 10240,
	})
	if err != nil {
		return nil, fmt.Errorf("clickhouse.Open: %w", err)
	}
	if err := conn.Ping(ctx); err != nil {
		return nil, fmt.Errorf("clickhouse ping: %w", err)
	}
	return &QueryServer{db: conn}, nil
}

// --- Middleware ---

func corsMiddleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		origin := r.Header.Get("Origin")
		if origin == "" {
			origin = "*"
		}
		w.Header().Set("Access-Control-Allow-Origin", origin)
		w.Header().Set("Access-Control-Allow-Methods", "GET, OPTIONS")
		w.Header().Set("Access-Control-Allow-Headers", "Content-Type, X-API-Key")
		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusNoContent)
			return
		}
		next.ServeHTTP(w, r)
	})
}

func apiKeyMiddleware(next http.HandlerFunc) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		expected := os.Getenv("SCOPE_API_KEY")
		if expected == "" {
			next(w, r)
			return
		}
		got := r.Header.Get("X-API-Key")
		if subtle.ConstantTimeCompare([]byte(got), []byte(expected)) != 1 {
			http.Error(w, `{"error":"unauthorized"}`, http.StatusUnauthorized)
			return
		}
		next(w, r)
	}
}

// --- Handlers ---

// queryWindow parses the `since` query param (a Go duration string, e.g.
// "15m", "1h") into an INTERVAL clause bound, defaulting to defaultWindow.
func queryWindow(r *http.Request, defaultWindow time.Duration) time.Duration {
	if raw := r.URL.Query().Get("since"); raw != "" {
		if d, err := time.ParseDuration(raw); err == nil && d > 0 {
			return d
		}
	}
	return defaultWindow
}

func (qs *QueryServer) handleTraces(w http.ResponseWriter, r *http.Request) {
	ctx, cancel := context.WithTimeout(r.Context(), 10*time.Second)
	defer cancel()

	window := queryWindow(r, time.Hour)
	service := r.URL.Query().Get("service")

	query := `
		SELECT span_id, operation_name, service_name,
		       toUnixTimestamp64Milli(start_time) AS start_ms,
		       duration_ns / 1000000.0            AS duration_ms,
		       parent_span_id
		FROM scope_traces
		WHERE start_time >= now() - toIntervalSecond(?)
	`
	args := []any{int64(window.Seconds())}
	if service != "" {
		query += " AND service_name = ?"
		args = append(args, service)
	}
	query += " ORDER BY start_time DESC LIMIT 200"

	rows, err := qs.db.Query(ctx, query, args...)
	if err != nil {
		http.Error(w, `{"error":"query failed"}`, http.StatusInternalServerError)
		log.Printf("traces query: %v", err)
		return
	}
	defer rows.Close()

	spans := make([]TraceSpan, 0)
	for rows.Next() {
		var s TraceSpan
		var parentID string
		if err := rows.Scan(&s.ID, &s.Name, &s.Service, &s.Start, &s.Duration, &parentID); err != nil {
			continue
		}
		if parentID != "" {
			p := parentID
			s.ParentID = &p
		}
		spans = append(spans, s)
	}
	writeJSON(w, spans)
}

func (qs *QueryServer) handleMetrics(w http.ResponseWriter, r *http.Request) {
	ctx, cancel := context.WithTimeout(r.Context(), 10*time.Second)
	defer cancel()

	window := queryWindow(r, time.Hour)
	service := r.URL.Query().Get("service")

	query := `
		SELECT metric_name,
		       toUnixTimestamp64Milli(timestamp) AS ts,
		       value
		FROM scope_metrics
		WHERE timestamp >= now() - toIntervalSecond(?)
	`
	args := []any{int64(window.Seconds())}
	if service != "" {
		query += " AND service_name = ?"
		args = append(args, service)
	}
	query += " ORDER BY metric_name, timestamp ASC LIMIT 10000"

	rows, err := qs.db.Query(ctx, query, args...)
	if err != nil {
		http.Error(w, `{"error":"query failed"}`, http.StatusInternalServerError)
		log.Printf("metrics query: %v", err)
		return
	}
	defer rows.Close()

	seriesMap := make(map[string]*MetricSeries)
	for rows.Next() {
		var name string
		var ts int64
		var val float64
		if err := rows.Scan(&name, &ts, &val); err != nil {
			continue
		}
		s, ok := seriesMap[name]
		if !ok {
			s = &MetricSeries{Name: name, Points: []MetricPoint{}}
			seriesMap[name] = s
		}
		s.Points = append(s.Points, MetricPoint{Timestamp: ts, Value: val})
	}

	series := make([]MetricSeries, 0, len(seriesMap))
	for _, s := range seriesMap {
		series = append(series, *s)
	}
	writeJSON(w, series)
}

func (qs *QueryServer) handleLogs(w http.ResponseWriter, r *http.Request) {
	ctx, cancel := context.WithTimeout(r.Context(), 10*time.Second)
	defer cancel()

	window := queryWindow(r, time.Hour)
	service := r.URL.Query().Get("service")

	query := `
		SELECT timestamp, service_name, level, message
		FROM scope_logs
		WHERE timestamp >= now() - toIntervalSecond(?)
	`
	args := []any{int64(window.Seconds())}
	if service != "" {
		query += " AND service_name = ?"
		args = append(args, service)
	}
	query += " ORDER BY timestamp DESC LIMIT 500"

	rows, err := qs.db.Query(ctx, query, args...)
	if err != nil {
		http.Error(w, `{"error":"query failed"}`, http.StatusInternalServerError)
		log.Printf("logs query: %v", err)
		return
	}
	defer rows.Close()

	entries := make([]LogEntry, 0)
	for rows.Next() {
		var ts time.Time
		var entry LogEntry
		if err := rows.Scan(&ts, &entry.Service, &entry.Severity, &entry.Message); err != nil {
			continue
		}
		entry.Time = ts.UTC().Format(time.RFC3339)
		entries = append(entries, entry)
	}
	writeJSON(w, entries)
}

// handleLogsStream streams new log entries as Server-Sent Events.
// Each event is a JSON-encoded LogEntry on a `data:` line.
func (qs *QueryServer) handleLogsStream(w http.ResponseWriter, r *http.Request) {
	flusher, ok := w.(http.Flusher)
	if !ok {
		http.Error(w, "streaming not supported", http.StatusInternalServerError)
		return
	}

	w.Header().Set("Content-Type", "text/event-stream")
	w.Header().Set("Cache-Control", "no-cache")
	w.Header().Set("Connection", "keep-alive")

	// Send connected event so the client knows the stream is live.
	fmt.Fprintf(w, "event: connected\ndata: {}\n\n")
	flusher.Flush()

	cursor := time.Now().UTC().Add(-5 * time.Second)
	ticker := time.NewTicker(2 * time.Second)
	defer ticker.Stop()

	for {
		select {
		case <-r.Context().Done():
			return
		case <-ticker.C:
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			rows, err := qs.db.Query(ctx, `
				SELECT timestamp, service_name, level, message
				FROM scope_logs
				WHERE timestamp > ?
				ORDER BY timestamp ASC
				LIMIT 100
			`, cursor)
			cancel()
			if err != nil {
				log.Printf("logs/stream query: %v", err)
				continue
			}
			for rows.Next() {
				var ts time.Time
				var entry LogEntry
				if err := rows.Scan(&ts, &entry.Service, &entry.Severity, &entry.Message); err != nil {
					continue
				}
				entry.Time = ts.UTC().Format(time.RFC3339)
				if ts.After(cursor) {
					cursor = ts
				}
				data, _ := json.Marshal(entry)
				fmt.Fprintf(w, "data: %s\n\n", data)
			}
			rows.Close()
			flusher.Flush()
		}
	}
}

// handleServices derives the service dependency graph shown by the dashboard
// from recent trace parent/child relationships, and marks each service's
// health from its recent log error rate.
func (qs *QueryServer) handleServices(w http.ResponseWriter, r *http.Request) {
	ctx, cancel := context.WithTimeout(r.Context(), 10*time.Second)
	defer cancel()

	names := make(map[string]struct{})
	deps := make(map[string]map[string]struct{})

	traceRows, err := qs.db.Query(ctx, `
		SELECT DISTINCT child.service_name, parent.service_name
		FROM scope_traces AS child
		INNER JOIN scope_traces AS parent ON child.parent_span_id = parent.span_id
		WHERE child.start_time >= now() - INTERVAL 1 HOUR
		  AND child.service_name != parent.service_name
		LIMIT 1000
	`)
	if err != nil {
		http.Error(w, `{"error":"query failed"}`, http.StatusInternalServerError)
		log.Printf("services (dependencies) query: %v", err)
		return
	}
	for traceRows.Next() {
		var childSvc, parentSvc string
		if err := traceRows.Scan(&childSvc, &parentSvc); err != nil {
			continue
		}
		names[childSvc] = struct{}{}
		names[parentSvc] = struct{}{}
		if deps[childSvc] == nil {
			deps[childSvc] = make(map[string]struct{})
		}
		deps[childSvc][parentSvc] = struct{}{}
	}
	traceRows.Close()

	// Include services that only appear as span roots or in logs/metrics so
	// isolated services still show up as graph nodes.
	nameRows, err := qs.db.Query(ctx, `
		SELECT DISTINCT service_name FROM scope_traces WHERE start_time >= now() - INTERVAL 1 HOUR
		UNION DISTINCT
		SELECT DISTINCT service_name FROM scope_logs WHERE timestamp >= now() - INTERVAL 1 HOUR
		LIMIT 1000
	`)
	if err != nil {
		http.Error(w, `{"error":"query failed"}`, http.StatusInternalServerError)
		log.Printf("services (names) query: %v", err)
		return
	}
	for nameRows.Next() {
		var name string
		if err := nameRows.Scan(&name); err != nil {
			continue
		}
		names[name] = struct{}{}
	}
	nameRows.Close()

	errorCounts := make(map[string]uint64)
	errRows, err := qs.db.Query(ctx, `
		SELECT service_name, count()
		FROM scope_logs
		WHERE timestamp >= now() - INTERVAL 5 MINUTE
		  AND upper(level) IN ('ERROR', 'FATAL', 'CRITICAL')
		GROUP BY service_name
	`)
	if err != nil {
		http.Error(w, `{"error":"query failed"}`, http.StatusInternalServerError)
		log.Printf("services (errors) query: %v", err)
		return
	}
	for errRows.Next() {
		var name string
		var count uint64
		if err := errRows.Scan(&name, &count); err != nil {
			continue
		}
		errorCounts[name] = count
	}
	errRows.Close()

	recentActivity := make(map[string]struct{})
	activityRows, err := qs.db.Query(ctx, `
		SELECT DISTINCT service_name FROM scope_logs WHERE timestamp >= now() - INTERVAL 5 MINUTE
	`)
	if err == nil {
		for activityRows.Next() {
			var name string
			if err := activityRows.Scan(&name); err == nil {
				recentActivity[name] = struct{}{}
			}
		}
		activityRows.Close()
	}

	services := make([]ServiceNode, 0, len(names))
	for name := range names {
		health := "healthy"
		if _, seenRecently := recentActivity[name]; !seenRecently {
			health = "down"
		} else if errorCounts[name] > 0 {
			health = "degraded"
		}

		depNames := make([]string, 0, len(deps[name]))
		for dep := range deps[name] {
			depNames = append(depNames, dep)
		}

		services = append(services, ServiceNode{
			Name:         name,
			Health:       health,
			Type:         "service",
			Dependencies: depNames,
		})
	}
	writeJSON(w, services)
}

func (qs *QueryServer) handleWasm(w http.ResponseWriter, r *http.Request) {
	ctx, cancel := context.WithTimeout(r.Context(), 10*time.Second)
	defer cancel()

	// Aggregate Wasm-specific metrics emitted by the Scope agent per service.
	rows, err := qs.db.Query(ctx, `
		SELECT service_name,
		       avgIf(value, metric_name = 'wasm.compile_time_ms')     AS compile_time,
		       avgIf(value, metric_name = 'wasm.instantiate_time_ms') AS instantiate_time,
		       maxIf(value, metric_name = 'wasm.memory_pages')        AS memory_pages
		FROM scope_metrics
		WHERE metric_name IN ('wasm.compile_time_ms', 'wasm.instantiate_time_ms', 'wasm.memory_pages')
		  AND timestamp >= now() - INTERVAL 1 HOUR
		GROUP BY service_name
		ORDER BY service_name
		LIMIT 100
	`)
	if err != nil {
		http.Error(w, `{"error":"query failed"}`, http.StatusInternalServerError)
		log.Printf("wasm query: %v", err)
		return
	}
	defer rows.Close()

	modules := make([]WasmModule, 0)
	for rows.Next() {
		var name string
		var compileTime, instantiateTime, memPages float64
		if err := rows.Scan(&name, &compileTime, &instantiateTime, &memPages); err != nil {
			continue
		}
		modules = append(modules, WasmModule{
			Name:            name,
			CompileTime:     fmt.Sprintf("%.1fms", compileTime),
			InstantiateTime: fmt.Sprintf("%.1fms", instantiateTime),
			MemoryPages:     int64(memPages),
			MemoryMB:        fmt.Sprintf("%.1f", memPages*64.0/1024.0), // 64 KiB per Wasm page
		})
	}
	writeJSON(w, modules)
}

// handleWasmInvocation fetches a single captured Wasm invocation by module
// name and wasm content sha256, for `origin replay` to re-run offline with
// the same args/env. Returns 404 if no matching invocation was captured.
func (qs *QueryServer) handleWasmInvocation(w http.ResponseWriter, r *http.Request) {
	ctx, cancel := context.WithTimeout(r.Context(), 10*time.Second)
	defer cancel()

	module := r.URL.Query().Get("module")
	sha256 := r.URL.Query().Get("sha256")
	if module == "" || sha256 == "" {
		http.Error(w, `{"error":"module and sha256 query params are required"}`, http.StatusBadRequest)
		return
	}

	row := qs.db.QueryRow(ctx, `
		SELECT toUnixTimestamp64Milli(timestamp), module_name, function, args_json, env_json, wasm_sha256, service_name, container_id
		FROM scope_wasm_invocations
		WHERE module_name = ? AND wasm_sha256 = ?
		ORDER BY timestamp DESC
		LIMIT 1`, module, sha256)

	var (
		timestampMs int64
		inv         WasmInvocation
		argsJSON    string
		envJSON     string
	)
	if err := row.Scan(&timestampMs, &inv.ModuleName, &inv.Function, &argsJSON, &envJSON, &inv.WasmSha256, &inv.ServiceName, &inv.ContainerID); err != nil {
		http.Error(w, `{"error":"no captured invocation found"}`, http.StatusNotFound)
		return
	}
	inv.Timestamp = fmt.Sprintf("%d", timestampMs)
	if err := json.Unmarshal([]byte(argsJSON), &inv.Args); err != nil {
		inv.Args = []string{}
	}
	if err := json.Unmarshal([]byte(envJSON), &inv.Env); err != nil {
		inv.Env = map[string]string{}
	}

	writeJSON(w, inv)
}

func (qs *QueryServer) handleHealth(w http.ResponseWriter, r *http.Request) {
	ctx, cancel := context.WithTimeout(r.Context(), 3*time.Second)
	defer cancel()

	status := "healthy"
	if err := qs.db.Ping(ctx); err != nil {
		status = "degraded: clickhouse unreachable"
		w.WriteHeader(http.StatusServiceUnavailable)
	}
	writeJSON(w, map[string]string{"status": status})
}

// --- Helpers ---

func writeJSON(w http.ResponseWriter, v any) {
	w.Header().Set("Content-Type", "application/json")
	if err := json.NewEncoder(w).Encode(v); err != nil {
		log.Printf("JSON encode: %v", err)
	}
}

func env(key, fallback string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return fallback
}

// --- Main ---

func main() {
	ctx := context.Background()

	if os.Getenv("SCOPE_API_KEY") == "" {
		log.Println("warning: SCOPE_API_KEY not set — query API is unauthenticated (dev mode)")
	}

	qs, err := newQueryServer(ctx)
	if err != nil {
		log.Fatalf("failed to connect to ClickHouse: %v", err)
	}

	mux := http.NewServeMux()
	mux.HandleFunc("GET /api/v1/services", apiKeyMiddleware(qs.handleServices))
	mux.HandleFunc("GET /api/v1/traces", apiKeyMiddleware(qs.handleTraces))
	mux.HandleFunc("GET /api/v1/metrics", apiKeyMiddleware(qs.handleMetrics))
	mux.HandleFunc("GET /api/v1/logs", apiKeyMiddleware(qs.handleLogs))
	mux.HandleFunc("GET /api/v1/logs/stream", apiKeyMiddleware(qs.handleLogsStream))
	mux.HandleFunc("GET /api/v1/wasm", apiKeyMiddleware(qs.handleWasm))
	mux.HandleFunc("GET /api/v1/wasm-invocations", apiKeyMiddleware(qs.handleWasmInvocation))
	mux.HandleFunc("GET /health", qs.handleHealth)
	mux.Handle("GET /metrics", MetricsHandler())

	// Rate limiting
	queryRateLimit := 200
	if env := os.Getenv("SCOPE_QUERY_RATE_LIMIT"); env != "" {
		if v, err := strconv.Atoi(env); err == nil && v > 0 {
			queryRateLimit = v
		}
	}
	rl := ratelimit.New(queryRateLimit, time.Minute)

	addr := env("SCOPE_BACKEND_ADDR", ":8081")
	srv := &http.Server{
		Addr:         addr,
		Handler:      rl.Middleware(corsMiddleware(instrumentMiddleware(mux))),
		ReadTimeout:  15 * time.Second,
		WriteTimeout: 0, // 0 = no limit; required for SSE streams
		IdleTimeout:  60 * time.Second,
	}

	log.Printf("TPT Scope Backend listening on %s", addr)
	log.Fatal(srv.ListenAndServe())
}
