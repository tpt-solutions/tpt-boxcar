package main

import (
	"encoding/json"
	"io"
	"net/http"
	"strings"

	"google.golang.org/protobuf/encoding/protojson"
	"google.golang.org/protobuf/proto"

	collectortrace "go.opentelemetry.io/proto/otlp/collector/trace/v1"
	collectormetrics "go.opentelemetry.io/proto/otlp/collector/metrics/v1"
	collectorlogs "go.opentelemetry.io/proto/otlp/collector/logs/v1"

	"github.com/tpt-cloud-native/scope/backend/internal/buffer"
	"github.com/tpt-cloud-native/scope/backend/internal/schema"
	"github.com/tpt-cloud-native/scope/backend/internal/transform"
)

type httpServer struct {
	traceRing   *buffer.RingBuffer[schema.TraceRecord]
	metricRing  *buffer.RingBuffer[schema.MetricRecord]
	logRing     *buffer.RingBuffer[schema.LogRecord]
}

func (s *httpServer) HandleTraces(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	body, err := io.ReadAll(r.Body)
	if err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		return
	}
	defer r.Body.Close()

	var req collectortrace.ExportTraceServiceRequest
	if err := decodeContent(r, body, &req); err != nil {
		http.Error(w, "decode error: "+err.Error(), http.StatusBadRequest)
		return
	}
	for _, rs := range req.GetResourceSpans() {
		resAttrs := rs.GetResource().GetAttributes()
		for _, ss := range rs.GetScopeSpans() {
			for _, span := range ss.GetSpans() {
				s.traceRing.Push(transform.SpanToRecord(span, resAttrs))
			}
		}
	}
	w.WriteHeader(http.StatusOK)
	json.NewEncoder(w).Encode(map[string]string{"status": "ok"})
}

func (s *httpServer) HandleMetrics(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	body, err := io.ReadAll(r.Body)
	if err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		return
	}
	defer r.Body.Close()

	var req collectormetrics.ExportMetricsServiceRequest
	if err := decodeContent(r, body, &req); err != nil {
		http.Error(w, "decode error: "+err.Error(), http.StatusBadRequest)
		return
	}
	for _, rm := range req.GetResourceMetrics() {
		resAttrs := rm.GetResource().GetAttributes()
		for _, sm := range rm.GetScopeMetrics() {
			for _, m := range sm.GetMetrics() {
				for _, rec := range transform.MetricToRecords(m, resAttrs) {
					s.metricRing.Push(rec)
				}
			}
		}
	}
	w.WriteHeader(http.StatusOK)
	json.NewEncoder(w).Encode(map[string]string{"status": "ok"})
}

func (s *httpServer) HandleLogs(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	body, err := io.ReadAll(r.Body)
	if err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		return
	}
	defer r.Body.Close()

	var req collectorlogs.ExportLogsServiceRequest
	if err := decodeContent(r, body, &req); err != nil {
		http.Error(w, "decode error: "+err.Error(), http.StatusBadRequest)
		return
	}
	for _, rl := range req.GetResourceLogs() {
		resAttrs := rl.GetResource().GetAttributes()
		for _, sl := range rl.GetScopeLogs() {
			for _, rec := range sl.GetLogRecords() {
				s.logRing.Push(transform.LogToRecord(rec, resAttrs))
			}
		}
	}
	w.WriteHeader(http.StatusOK)
	json.NewEncoder(w).Encode(map[string]string{"status": "ok"})
}

// decodeContent unmarshals the request body based on Content-Type.
// Supports application/x-protobuf, application/json, and application/proto.
func decodeContent(r *http.Request, body []byte, msg proto.Message) error {
	ct := r.Header.Get("Content-Type")
	if strings.Contains(ct, "json") {
		return protojson.Unmarshal(body, msg)
	}
	return proto.Unmarshal(body, msg)
}
