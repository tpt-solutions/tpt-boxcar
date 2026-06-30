package transform

import (
	"encoding/hex"
	"encoding/json"
	"time"

	commonpb "go.opentelemetry.io/proto/otlp/common/v1"
	tracepb "go.opentelemetry.io/proto/otlp/trace/v1"

	"github.com/tpt-cloud-native/scope/backend/internal/schema"
)

// SpanToRecord converts an OTLP span plus resource attributes into a TraceRecord.
func SpanToRecord(span *tracepb.Span, resource []*commonpb.KeyValue) schema.TraceRecord {
	attrs := extractResourceAttrs(resource)
	spanAttrs := extractKV(span.GetAttributes())
	mergeMap(attrs, spanAttrs)

	return schema.TraceRecord{
		TraceID:       hex.EncodeToString(span.GetTraceId()),
		SpanID:        hex.EncodeToString(span.GetSpanId()),
		ParentSpanID:  hex.EncodeToString(span.GetParentSpanId()),
		ServiceName:   attrs["service.name"],
		OperationName: span.GetName(),
		StartTime:     time.Unix(0, int64(span.GetStartTimeUnixNano())),
		EndTime:       time.Unix(0, int64(span.GetEndTimeUnixNano())),
		DurationNs:    span.GetEndTimeUnixNano() - span.GetStartTimeUnixNano(),
		Status:        statusString(span.GetStatus().GetCode()),
		StatusMessage: span.GetStatus().GetMessage(),
		SpanKind:      kindString(span.GetKind()),
		ContainerID:   attrs["container.id"],
		ImageName:     attrs["container.image.name"],
		PodName:       attrs["k8s.pod.name"],
		Namespace:     attrs["k8s.namespace.name"],
		NodeName:      attrs["k8s.node.name"],
		Attributes:    spanAttrs,
		Events:        marshalEvents(span.GetEvents()),
	}
}

func extractResourceAttrs(resources []*commonpb.KeyValue) map[string]string {
	out := make(map[string]string)
	for _, r := range resources {
		out[r.GetKey()] = anyValueString(r.GetValue())
	}
	return out
}

func extractKV(kvs []*commonpb.KeyValue) map[string]string {
	out := make(map[string]string)
	for _, kv := range kvs {
		out[kv.GetKey()] = anyValueString(kv.GetValue())
	}
	return out
}

func anyValueString(av *commonpb.AnyValue) string {
	if av == nil {
		return ""
	}
	switch v := av.GetValue().(type) {
	case *commonpb.AnyValue_StringValue:
		return v.StringValue
	case *commonpb.AnyValue_BoolValue:
		if v.BoolValue {
			return "true"
		}
		return "false"
	case *commonpb.AnyValue_IntValue:
		return fmtInt(v.IntValue)
	case *commonpb.AnyValue_DoubleValue:
		return fmtFloat(v.DoubleValue)
	case *commonpb.AnyValue_KvlistValue:
		b, _ := json.Marshal(v.KvlistValue.GetValues())
		return string(b)
	case *commonpb.AnyValue_ArrayValue:
		b, _ := json.Marshal(v.ArrayValue.GetValues())
		return string(b)
	case *commonpb.AnyValue_BytesValue:
		return hex.EncodeToString(v.BytesValue)
	default:
		return ""
	}
}

func statusString(code tracepb.Status_StatusCode) string {
	switch code {
	case tracepb.Status_STATUS_CODE_OK:
		return "OK"
	case tracepb.Status_STATUS_CODE_ERROR:
		return "ERROR"
	default:
		return "UNSET"
	}
}

func kindString(k tracepb.Span_SpanKind) string {
	switch k {
	case tracepb.Span_SPAN_KIND_SERVER:
		return "server"
	case tracepb.Span_SPAN_KIND_CLIENT:
		return "client"
	case tracepb.Span_SPAN_KIND_PRODUCER:
		return "producer"
	case tracepb.Span_SPAN_KIND_CONSUMER:
		return "consumer"
	case tracepb.Span_SPAN_KIND_INTERNAL:
		return "internal"
	default:
		return "unspecified"
	}
}

func marshalEvents(events []*tracepb.Span_Event) string {
	if len(events) == 0 {
		return ""
	}
	type simpleEvent struct {
		Time       uint64            `json:"time"`
		Name       string            `json:"name"`
		Attributes map[string]string `json:"attributes,omitempty"`
	}
	out := make([]simpleEvent, 0, len(events))
	for _, e := range events {
		out = append(out, simpleEvent{
			Time:       e.GetTimeUnixNano(),
			Name:       e.GetName(),
			Attributes: extractKV(e.GetAttributes()),
		})
	}
	b, _ := json.Marshal(out)
	return string(b)
}
