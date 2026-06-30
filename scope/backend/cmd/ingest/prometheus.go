package main

import (
	"github.com/prometheus/client_golang/prometheus"
	"github.com/prometheus/client_golang/prometheus/promhttp"
	"net/http"
)

var (
	tracesIngested = prometheus.NewCounter(prometheus.CounterOpts{
		Name: "scope_ingest_traces_total",
		Help: "Total number of trace spans received via OTLP",
	})
	metricsIngested = prometheus.NewCounter(prometheus.CounterOpts{
		Name: "scope_ingest_metrics_total",
		Help: "Total number of metric data points received via OTLP",
	})
	logsIngested = prometheus.NewCounter(prometheus.CounterOpts{
		Name: "scope_ingest_logs_total",
		Help: "Total number of log records received via OTLP",
	})
	traceBufferDepth = prometheus.NewGauge(prometheus.GaugeOpts{
		Name: "scope_ingest_trace_buffer_depth",
		Help: "Current number of trace records in the ring buffer",
	})
	metricBufferDepth = prometheus.NewGauge(prometheus.GaugeOpts{
		Name: "scope_ingest_metric_buffer_depth",
		Help: "Current number of metric records in the ring buffer",
	})
	logBufferDepth = prometheus.NewGauge(prometheus.GaugeOpts{
		Name: "scope_ingest_log_buffer_depth",
		Help: "Current number of log records in the ring buffer",
	})
)

func init() {
	prometheus.MustRegister(
		tracesIngested, metricsIngested, logsIngested,
		traceBufferDepth, metricBufferDepth, logBufferDepth,
	)
}

// MetricsHandler serves the /metrics endpoint.
func MetricsHandler() http.Handler {
	return promhttp.Handler()
}
