package main

import (
	"context"

	collectortrace "go.opentelemetry.io/proto/otlp/collector/trace/v1"
	collectormetrics "go.opentelemetry.io/proto/otlp/collector/metrics/v1"
	collectorlogs "go.opentelemetry.io/proto/otlp/collector/logs/v1"

	"github.com/tpt-boxcar/scope/backend/internal/buffer"
	"github.com/tpt-boxcar/scope/backend/internal/schema"
	"github.com/tpt-boxcar/scope/backend/internal/transform"
)

type traceServer struct {
	collectortrace.UnimplementedTraceServiceServer
	ring *buffer.RingBuffer[schema.TraceRecord]
}

func (s *traceServer) Export(_ context.Context, req *collectortrace.ExportTraceServiceRequest) (*collectortrace.ExportTraceServiceResponse, error) {
	for _, rs := range req.GetResourceSpans() {
		resAttrs := rs.GetResource().GetAttributes()
		for _, ss := range rs.GetScopeSpans() {
			for _, span := range ss.GetSpans() {
				s.ring.Push(transform.SpanToRecord(span, resAttrs))
			}
		}
	}
	return &collectortrace.ExportTraceServiceResponse{}, nil
}

type metricsServer struct {
	collectormetrics.UnimplementedMetricsServiceServer
	ring *buffer.RingBuffer[schema.MetricRecord]
}

func (s *metricsServer) Export(_ context.Context, req *collectormetrics.ExportMetricsServiceRequest) (*collectormetrics.ExportMetricsServiceResponse, error) {
	for _, rm := range req.GetResourceMetrics() {
		resAttrs := rm.GetResource().GetAttributes()
		for _, sm := range rm.GetScopeMetrics() {
			for _, m := range sm.GetMetrics() {
				for _, rec := range transform.MetricToRecords(m, resAttrs) {
					s.ring.Push(rec)
				}
			}
		}
	}
	return &collectormetrics.ExportMetricsServiceResponse{}, nil
}

type logServer struct {
	collectorlogs.UnimplementedLogsServiceServer
	ring *buffer.RingBuffer[schema.LogRecord]
}

func (s *logServer) Export(_ context.Context, req *collectorlogs.ExportLogsServiceRequest) (*collectorlogs.ExportLogsServiceResponse, error) {
	for _, rl := range req.GetResourceLogs() {
		resAttrs := rl.GetResource().GetAttributes()
		for _, sl := range rl.GetScopeLogs() {
			for _, rec := range sl.GetLogRecords() {
				s.ring.Push(transform.LogToRecord(rec, resAttrs))
			}
		}
	}
	return &collectorlogs.ExportLogsServiceResponse{}, nil
}
