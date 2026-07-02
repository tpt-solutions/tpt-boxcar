package transform

import (
	"fmt"
	"time"

	commonpb "go.opentelemetry.io/proto/otlp/common/v1"
	metricpb "go.opentelemetry.io/proto/otlp/metrics/v1"

	"github.com/tpt-boxcar/scope/backend/internal/schema"
)

// MetricToRecords converts an OTLP Metric plus resource attributes into one or more MetricRecords.
func MetricToRecords(metric *metricpb.Metric, resource []*commonpb.KeyValue) []schema.MetricRecord {
	attrs := extractResourceAttrs(resource)
	serviceName := attrs["service.name"]
	containerID := attrs["container.id"]
	imageName := attrs["container.image.name"]
	podName := attrs["k8s.pod.name"]
	namespace := attrs["k8s.namespace.name"]
	nodeName := attrs["k8s.node.name"]

	switch m := metric.GetData().(type) {
	case *metricpb.Metric_Gauge:
		return gaugeRecords(m.Gauge, metric.GetName(), serviceName, containerID, imageName, podName, namespace, nodeName)
	case *metricpb.Metric_Sum:
		return sumRecords(m.Sum, metric.GetName(), serviceName, containerID, imageName, podName, namespace, nodeName)
	case *metricpb.Metric_Histogram:
		return histogramRecords(m.Histogram, metric.GetName(), serviceName, containerID, imageName, podName, namespace, nodeName)
	default:
		return nil
	}
}

func gaugeRecords(gauge *metricpb.Gauge, name, service, container, image, pod, ns, node string) []schema.MetricRecord {
	var out []schema.MetricRecord
	for _, dp := range gauge.GetDataPoints() {
		out = append(out, schema.MetricRecord{
			MetricName:  name,
			MetricType:  "gauge",
			Timestamp:   time.Unix(0, int64(dp.GetTimeUnixNano())),
			Value:       dp.GetAsDouble(),
			Labels:      extractKV(dp.GetAttributes()),
			ServiceName: service,
			ContainerID: container,
			ImageName:   image,
			PodName:     pod,
			Namespace:   ns,
			NodeName:    node,
		})
	}
	return out
}

func sumRecords(sum *metricpb.Sum, name, service, container, image, pod, ns, node string) []schema.MetricRecord {
	var out []schema.MetricRecord
	for _, dp := range sum.GetDataPoints() {
		out = append(out, schema.MetricRecord{
			MetricName:  name,
			MetricType:  metricTypeString(sum.GetIsMonotonic(), true),
			Timestamp:   time.Unix(0, int64(dp.GetTimeUnixNano())),
			Value:       dp.GetAsDouble(),
			Labels:      extractKV(dp.GetAttributes()),
			ServiceName: service,
			ContainerID: container,
			ImageName:   image,
			PodName:     pod,
			Namespace:   ns,
			NodeName:    node,
		})
	}
	return out
}

func histogramRecords(h *metricpb.Histogram, name, service, container, image, pod, ns, node string) []schema.MetricRecord {
	var out []schema.MetricRecord
	for _, dp := range h.GetDataPoints() {
		out = append(out, schema.MetricRecord{
			MetricName:  name,
			MetricType:  "histogram",
			Timestamp:   time.Unix(0, int64(dp.GetTimeUnixNano())),
			Value:       dp.GetSum(),
			Labels:      extractKV(dp.GetAttributes()),
			ServiceName: service,
			ContainerID: container,
			ImageName:   image,
			PodName:     pod,
			Namespace:   ns,
			NodeName:    node,
		})
	}
	return out
}

func metricTypeString(monotonic, isSum bool) string {
	if isSum && monotonic {
		return "counter"
	}
	return "sum"
}

func mergeMap(dst, src map[string]string) {
	for k, v := range src {
		if _, exists := dst[k]; !exists {
			dst[k] = v
		}
	}
}

func fmtInt(v int64) string  { return fmt.Sprintf("%d", v) }
func fmtFloat(v float64) string { return fmt.Sprintf("%g", v) }
