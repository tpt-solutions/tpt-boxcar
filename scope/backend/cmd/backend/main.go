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

type WasmModule struct {
	Name            string `json:"name"`
	CompileTime     string `json:"compileTime"`
	InstantiateTime string `json:"instantiateTime"`
	MemoryPages     int64  `json:"memoryPages"`
	MemoryMB        string `json:"memoryMB"`
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

func (qs *QueryServer) handleTraces(w http.ResponseWriter, r *http.Request) {
	ctx, cancel := context.WithTimeout(r.Context(), 10*time.Second)
	defer cancel()

	rows, err := qs.db.Query(ctx, `
		SELECT span_id, operation_name, service_name,
		       toUnixTimestamp64Milli(start_time) AS start_ms,
		       duration_ns / 1000000.0            AS duration_ms,
		       parent_span_id
		FROM scope_traces
		ORDER BY start_time DESC
		LIMIT 200
	`)
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

	rows, err := qs.db.Query(ctx, `
		SELECT metric_name,
		       toUnixTimestamp64Milli(timestamp) AS ts,
		       value
		FROM scope_metrics
		WHERE timestamp >= now() - INTERVAL 1 HOUR
		ORDER BY metric_name, timestamp ASC
		LIMIT 10000
	`)
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

	rows, err := qs.db.Query(ctx, `
		SELECT timestamp, service_name, level, message
		FROM scope_logs
		ORDER BY timestamp DESC
		LIMIT 500
	`)
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
	mux.HandleFunc("GET /api/v1/traces", apiKeyMiddleware(qs.handleTraces))
	mux.HandleFunc("GET /api/v1/metrics", apiKeyMiddleware(qs.handleMetrics))
	mux.HandleFunc("GET /api/v1/logs", apiKeyMiddleware(qs.handleLogs))
	mux.HandleFunc("GET /api/v1/logs/stream", apiKeyMiddleware(qs.handleLogsStream))
	mux.HandleFunc("GET /api/v1/wasm", apiKeyMiddleware(qs.handleWasm))
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
