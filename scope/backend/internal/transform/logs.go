package transform

import (
	"time"

	commonpb "go.opentelemetry.io/proto/otlp/common/v1"
	logpb "go.opentelemetry.io/proto/otlp/logs/v1"

	"github.com/tpt-cloud-native/scope/backend/internal/schema"
)

// LogToRecord converts an OTLP LogRecord plus resource attributes into a LogRecord.
func LogToRecord(rec *logpb.LogRecord, resource []*commonpb.KeyValue) schema.LogRecord {
	attrs := extractResourceAttrs(resource)
	spanAttrs := extractKV(rec.GetAttributes())

	return schema.LogRecord{
		Timestamp:   time.Unix(0, int64(rec.GetTimeUnixNano())),
		Level:       severityString(rec.GetSeverityNumber()),
		ServiceName: attrs["service.name"],
		Message:     bodyString(rec.GetBody()),
		TraceID:     hexEncodeBytes(rec.GetTraceId()),
		SpanID:      hexEncodeBytes(rec.GetSpanId()),
		ContainerID: attrs["container.id"],
		ImageName:   attrs["container.image.name"],
		PodName:     attrs["k8s.pod.name"],
		Namespace:   attrs["k8s.namespace.name"],
		NodeName:    attrs["k8s.node.name"],
		Attributes:  spanAttrs,
	}
}

func severityString(sev logpb.SeverityNumber) string {
	switch sev {
	case logpb.SeverityNumber_SEVERITY_NUMBER_TRACE:
		return "TRACE"
	case logpb.SeverityNumber_SEVERITY_NUMBER_TRACE2:
		return "TRACE2"
	case logpb.SeverityNumber_SEVERITY_NUMBER_TRACE3:
		return "TRACE3"
	case logpb.SeverityNumber_SEVERITY_NUMBER_TRACE4:
		return "TRACE4"
	case logpb.SeverityNumber_SEVERITY_NUMBER_DEBUG:
		return "DEBUG"
	case logpb.SeverityNumber_SEVERITY_NUMBER_DEBUG2:
		return "DEBUG2"
	case logpb.SeverityNumber_SEVERITY_NUMBER_DEBUG3:
		return "DEBUG3"
	case logpb.SeverityNumber_SEVERITY_NUMBER_DEBUG4:
		return "DEBUG4"
	case logpb.SeverityNumber_SEVERITY_NUMBER_INFO:
		return "INFO"
	case logpb.SeverityNumber_SEVERITY_NUMBER_INFO2:
		return "INFO2"
	case logpb.SeverityNumber_SEVERITY_NUMBER_INFO3:
		return "INFO3"
	case logpb.SeverityNumber_SEVERITY_NUMBER_INFO4:
		return "INFO4"
	case logpb.SeverityNumber_SEVERITY_NUMBER_WARN:
		return "WARN"
	case logpb.SeverityNumber_SEVERITY_NUMBER_WARN2:
		return "WARN2"
	case logpb.SeverityNumber_SEVERITY_NUMBER_WARN3:
		return "WARN3"
	case logpb.SeverityNumber_SEVERITY_NUMBER_WARN4:
		return "WARN4"
	case logpb.SeverityNumber_SEVERITY_NUMBER_ERROR:
		return "ERROR"
	case logpb.SeverityNumber_SEVERITY_NUMBER_ERROR2:
		return "ERROR2"
	case logpb.SeverityNumber_SEVERITY_NUMBER_ERROR3:
		return "ERROR3"
	case logpb.SeverityNumber_SEVERITY_NUMBER_ERROR4:
		return "ERROR4"
	case logpb.SeverityNumber_SEVERITY_NUMBER_FATAL:
		return "FATAL"
	case logpb.SeverityNumber_SEVERITY_NUMBER_FATAL2:
		return "FATAL2"
	case logpb.SeverityNumber_SEVERITY_NUMBER_FATAL3:
		return "FATAL3"
	case logpb.SeverityNumber_SEVERITY_NUMBER_FATAL4:
		return "FATAL4"
	default:
		return "UNSPECIFIED"
	}
}

func bodyString(body *commonpb.AnyValue) string {
	if body == nil {
		return ""
	}
	return anyValueString(body)
}

func hexEncodeBytes(b []byte) string {
	if len(b) == 0 {
		return ""
	}
	out := make([]byte, len(b)*2)
	for i, c := range b {
		out[i*2] = "0123456789abcdef"[c>>4]
		out[i*2+1] = "0123456789abcdef"[c&0x0f]
	}
	return string(out)
}
