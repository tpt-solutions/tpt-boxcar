package main

import (
	"net/http"
	"strconv"
	"time"

	"github.com/prometheus/client_golang/prometheus"
	"github.com/prometheus/client_golang/prometheus/promhttp"
)

var (
	httpRequestsTotal = prometheus.NewCounterVec(
		prometheus.CounterOpts{
			Name: "tpt_http_requests_total",
			Help: "Total number of HTTP requests by method, path, and status code",
		},
		[]string{"method", "path", "status"},
	)
	httpRequestDuration = prometheus.NewHistogramVec(
		prometheus.HistogramOpts{
			Name:    "tpt_http_request_duration_seconds",
			Help:    "Histogram of HTTP request durations in seconds",
			Buckets: prometheus.DefBuckets,
		},
		[]string{"method", "path"},
	)
	httpInflightRequests = prometheus.NewGauge(
		prometheus.GaugeOpts{
			Name: "tpt_http_inflight_requests",
			Help: "Current number of in-flight HTTP requests",
		},
	)
)

func init() {
	prometheus.MustRegister(httpRequestsTotal, httpRequestDuration, httpInflightRequests)
}

// MetricsHandler serves the /metrics endpoint.
func MetricsHandler() http.Handler {
	return promhttp.Handler()
}

// instrumentMiddleware wraps an http.Handler with Prometheus HTTP metrics.
func instrumentMiddleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		httpInflightRequests.Inc()
		defer httpInflightRequests.Dec()

		start := time.Now()

		// Wrap response writer to capture status code.
		lrw := &loggingResponseWriter{ResponseWriter: w, statusCode: http.StatusOK}
		next.ServeHTTP(lrw, r)

		duration := time.Since(start).Seconds()
		status := strconv.Itoa(lrw.statusCode)
		// Use URL pattern path — r.URL.Path is sufficient for ServeMux routing.
		httpRequestsTotal.WithLabelValues(r.Method, r.URL.Path, status).Inc()
		httpRequestDuration.WithLabelValues(r.Method, r.URL.Path).Observe(duration)
	})
}

// loggingResponseWriter captures the HTTP status code.
type loggingResponseWriter struct {
	http.ResponseWriter
	statusCode int
}

func (lrw *loggingResponseWriter) WriteHeader(code int) {
	lrw.statusCode = code
	lrw.ResponseWriter.WriteHeader(code)
}