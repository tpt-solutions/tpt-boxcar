package main

import (
	"fmt"
	"net/http"
	"sort"
	"strings"
	"sync/atomic"
	"time"
)

// Metrics provides a lightweight Prometheus-format /metrics endpoint
// with no external dependencies. Uses atomic counters for thread safety.
type Metrics struct {
	// requestCount is keyed by "method:path:status"
	requestCount atomicCounterMap
	// latencyBuckets stores cumulative counts per bucket (seconds)
	latencyBuckets latencyHistogram
	// inflight is the current number of in-flight requests
	inflight atomic.Int64
}

type atomicCounterMap struct {
	mu   chan struct{}
	data map[string]*atomic.Int64
}

func newAtomicCounterMap() atomicCounterMap {
	return atomicCounterMap{
		mu:   make(chan struct{}, 1),
		data: make(map[string]*atomic.Int64),
	}
}

func (m *atomicCounterMap) inc(key string) {
	m.mu <- struct{}{}
	c, ok := m.data[key]
	if !ok {
		c = new(atomic.Int64)
		m.data[key] = c
	}
	<-m.mu
	c.Add(1)
}

func (m *atomicCounterMap) snapshot() map[string]int64 {
	m.mu <- struct{}{}
	out := make(map[string]int64, len(m.data))
	for k, c := range m.data {
		out[k] = c.Load()
	}
	<-m.mu
	return out
}

// latencyHistogram uses Prometheus default buckets: .005, .01, .025, .05, .1, .25, .5, 1, 2.5, 5, 10
type latencyHistogram struct {
	mu      chan struct{}
	buckets []float64
	counts  []*atomic.Int64
	total   atomic.Int64
	sum     atomic.Float64
}

func newLatencyHistogram(buckets []float64) latencyHistogram {
	counts := make([]*atomic.Int64, len(buckets))
	for i := range counts {
		counts[i] = new(atomic.Int64)
	}
	return latencyHistogram{
		mu:      make(chan struct{}, 1),
		buckets: buckets,
		counts:  counts,
	}
}

func (h *latencyHistogram) observe(seconds float64) {
	h.total.Add(1)
	h.sum.Add(seconds)
	for i, b := range h.buckets {
		if seconds <= b {
			h.counts[i].Add(1)
			return
		}
	}
	// Falls into the last bucket (i.e. +Inf)
}

func (h *latencyHistogram) snapshot() (total int64, sum float64, counts []int64) {
	total = h.total.Load()
	sum = h.sum.Load()
	counts = make([]int64, len(h.counts))
	for i, c := range h.counts {
		counts[i] = c.Load()
	}
	return
}

// NewMetrics creates a Metrics instance with default latency buckets.
func NewMetrics() *Metrics {
	return &Metrics{
		requestCount: newAtomicCounterMap(),
		latencyBuckets: newLatencyHistogram([]float64{
			0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5, 10,
		}),
	}
}

// Instrument wraps an http.Handler with metrics collection.
func (m *Metrics) Instrument(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		m.inflight.Add(1)
		defer m.inflight.Add(-1)

		start := time.Now()
		lrw := &metricsResponseWriter{ResponseWriter: w, statusCode: http.StatusOK}
		next.ServeHTTP(lrw, r)

		duration := time.Since(start).Seconds()
		key := fmt.Sprintf("%s:%s:%d", r.Method, r.URL.Path, lrw.statusCode)
		m.requestCount.inc(key)
		m.latencyBuckets.observe(duration)
	})
}

type metricsResponseWriter struct {
	http.ResponseWriter
	statusCode int
}

func (w *metricsResponseWriter) WriteHeader(code int) {
	w.statusCode = code
	w.ResponseWriter.WriteHeader(code)
}

// ServeHTTP serves the /metrics endpoint in Prometheus text format.
func (m *Metrics) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "text/plain; version=0.0.4")
	w.WriteHeader(http.StatusOK)

	var sb strings.Builder

	// --- tpt_http_requests_total ---
	sb.WriteString("# HELP tpt_http_requests_total Total number of HTTP requests by method, path, and status code\n")
	sb.WriteString("# TYPE tpt_http_requests_total counter\n")
	counts := m.requestCount.snapshot()
	keys := make([]string, 0, len(counts))
	for k := range counts {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	for _, k := range keys {
		parts := strings.SplitN(k, ":", 3)
		if len(parts) == 3 {
			sb.WriteString(fmt.Sprintf("tpt_http_requests_total{method=%q,path=%q,status=%q} %d\n",
				parts[0], parts[1], parts[2], counts[k]))
		}
	}

	// --- tpt_http_request_duration_seconds ---
	total, sum, bucketCounts := m.latencyBuckets.snapshot()
	sb.WriteString("# HELP tpt_http_request_duration_seconds Histogram of HTTP request durations in seconds\n")
	sb.WriteString("# TYPE tpt_http_request_duration_seconds histogram\n")
	sb.WriteString(fmt.Sprintf("tpt_http_request_duration_seconds_count %d\n", total))
	sb.WriteString(fmt.Sprintf("tpt_http_request_duration_seconds_sum %.9f\n", sum))
	for i, b := range m.latencyBuckets.buckets {
		sb.WriteString(fmt.Sprintf("tpt_http_request_duration_seconds_bucket{le=%q} %d\n",
			fmt.Sprintf("%.9f", b), bucketCounts[i]))
	}
	sb.WriteString(fmt.Sprintf("tpt_http_request_duration_seconds_bucket{le=%q} %d\n", "+Inf", total))

	// --- tpt_http_inflight_requests ---
	sb.WriteString("# HELP tpt_http_inflight_requests Current number of in-flight HTTP requests\n")
	sb.WriteString("# TYPE tpt_http_inflight_requests gauge\n")
	sb.WriteString(fmt.Sprintf("tpt_http_inflight_requests %d\n", m.inflight.Load()))

	w.Write([]byte(sb.String()))
}