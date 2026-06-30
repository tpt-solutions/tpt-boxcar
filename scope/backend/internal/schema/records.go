package schema

import "time"

// TraceRecord represents a single span within a distributed trace.
type TraceRecord struct {
	TraceID       string            `json:"trace_id"`
	SpanID        string            `json:"span_id"`
	ParentSpanID  string            `json:"parent_span_id"`
	ServiceName   string            `json:"service_name"`
	OperationName string            `json:"operation_name"`
	StartTime     time.Time         `json:"start_time"`
	EndTime       time.Time         `json:"end_time"`
	DurationNs    uint64            `json:"duration_ns"`
	Status        string            `json:"status"`
	StatusMessage string            `json:"status_message"`
	SpanKind      string            `json:"span_kind"`
	ContainerID   string            `json:"container_id"`
	ImageName     string            `json:"image_name"`
	PodName       string            `json:"pod_name"`
	Namespace     string            `json:"namespace"`
	NodeName      string            `json:"node_name"`
	Attributes    map[string]string `json:"attributes"`
	Events        string            `json:"events"`
}

// MetricRecord represents a single data point from a metric.
type MetricRecord struct {
	MetricName  string            `json:"metric_name"`
	MetricType  string            `json:"metric_type"`
	Timestamp   time.Time         `json:"timestamp"`
	Value       float64           `json:"value"`
	Labels      map[string]string `json:"labels"`
	ServiceName string            `json:"service_name"`
	ContainerID string            `json:"container_id"`
	ImageName   string            `json:"image_name"`
	PodName     string            `json:"pod_name"`
	Namespace   string            `json:"namespace"`
	NodeName    string            `json:"node_name"`
}

// LogRecord represents a single log entry.
type LogRecord struct {
	Timestamp   time.Time         `json:"timestamp"`
	Level       string            `json:"level"`
	ServiceName string            `json:"service_name"`
	Message     string            `json:"message"`
	TraceID     string            `json:"trace_id"`
	SpanID      string            `json:"span_id"`
	ContainerID string            `json:"container_id"`
	ImageName   string            `json:"image_name"`
	PodName     string            `json:"pod_name"`
	Namespace   string            `json:"namespace"`
	NodeName    string            `json:"node_name"`
	Attributes  map[string]string `json:"attributes"`
}
