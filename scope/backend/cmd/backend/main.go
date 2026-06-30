package main

import (
	"encoding/json"
	"fmt"
	"log"
	"net/http"
	"sync"
)

type Trace struct {
	TraceID  string   `json:"trace_id"`
	Services []string `json:"services"`
	Duration float64  `json:"duration_ms"`
}

type Metric struct {
	Name      string            `json:"name"`
	Value     float64           `json:"value"`
	Labels    map[string]string `json:"labels"`
	Timestamp int64             `json:"timestamp"`
}

type LogEntry struct {
	Timestamp string `json:"timestamp"`
	Service   string `json:"service"`
	Severity  string `json:"severity"`
	Message   string `json:"message"`
}

type QueryServer struct {
	mu      sync.RWMutex
	traces  []Trace
	metrics []Metric
	logs    []LogEntry
}

func NewQueryServer() *QueryServer {
	return &QueryServer{}
}

func (qs *QueryServer) handleTraces(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")
	qs.mu.RLock()
	defer qs.mu.RUnlock()
	json.NewEncoder(w).Encode(qs.traces)
}

func (qs *QueryServer) handleMetrics(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")
	qs.mu.RLock()
	defer qs.mu.RUnlock()
	json.NewEncoder(w).Encode(qs.metrics)
}

func (qs *QueryServer) handleLogs(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")
	qs.mu.RLock()
	defer qs.mu.RUnlock()
	json.NewEncoder(w).Encode(qs.logs)
}

func (qs *QueryServer) handleHealth(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]string{"status": "healthy"})
}

func main() {
	qs := NewQueryServer()

	mux := http.NewServeMux()
	mux.HandleFunc("/api/v1/traces", qs.handleTraces)
	mux.HandleFunc("/api/v1/metrics", qs.handleMetrics)
	mux.HandleFunc("/api/v1/logs", qs.handleLogs)
	mux.HandleFunc("/health", qs.handleHealth)

	addr := ":8081"
	fmt.Printf("TPT Scope Backend listening on %s\n", addr)
	log.Fatal(http.ListenAndServe(addr, mux))
}
