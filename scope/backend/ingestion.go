package main

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"sync"
	"time"
)

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

type BatchBuffer[T any] struct {
	mu          sync.Mutex
	items       []T
	batchSize   int
	flushFn     func([]T)
	flushTicker *time.Ticker
	done        chan struct{}
}

func NewBatchBuffer[T any](batchSize int, flushInterval time.Duration, flushFn func([]T)) *BatchBuffer[T] {
	bb := &BatchBuffer[T]{
		items:       make([]T, 0, batchSize),
		batchSize:   batchSize,
		flushFn:     flushFn,
		flushTicker: time.NewTicker(flushInterval),
		done:        make(chan struct{}),
	}
	go bb.backgroundFlush()
	return bb
}

func (bb *BatchBuffer[T]) Add(item T) {
	bb.mu.Lock()
	bb.items = append(bb.items, item)
	shouldFlush := len(bb.items) >= bb.batchSize
	bb.mu.Unlock()

	if shouldFlush {
		bb.Flush()
	}
}

func (bb *BatchBuffer[T]) Flush() {
	bb.mu.Lock()
	if len(bb.items) == 0 {
		bb.mu.Unlock()
		return
	}
	batch := bb.items
	bb.items = make([]T, 0, bb.batchSize)
	bb.mu.Unlock()

	bb.flushFn(batch)
}

func (bb *BatchBuffer[T]) Len() int {
	bb.mu.Lock()
	defer bb.mu.Unlock()
	return len(bb.items)
}

func (bb *BatchBuffer[T]) backgroundFlush() {
	for {
		select {
		case <-bb.flushTicker.C:
			bb.Flush()
		case <-bb.done:
			return
		}
	}
}

func (bb *BatchBuffer[T]) Stop() {
	bb.flushTicker.Stop()
	close(bb.done)
}

type IngestionPipeline struct {
	clickhouseURL string
	httpClient    *http.Client
	traceBuffer   *BatchBuffer[TraceRecord]
	metricBuffer  *BatchBuffer[MetricRecord]
	logBuffer     *BatchBuffer[LogRecord]
}

type IngestionConfig struct {
	ClickhouseURL  string
	BatchSize      int
	FlushInterval  time.Duration
	HTTPTimeout    time.Duration
}

func DefaultIngestionConfig() IngestionConfig {
	return IngestionConfig{
		ClickhouseURL: "http://localhost:8123",
		BatchSize:     1000,
		FlushInterval: 5 * time.Second,
		HTTPTimeout:   30 * time.Second,
	}
}

func NewIngestionPipeline(cfg IngestionConfig) *IngestionPipeline {
	p := &IngestionPipeline{
		clickhouseURL: cfg.ClickhouseURL,
		httpClient: &http.Client{
			Timeout: cfg.HTTPTimeout,
		},
	}

	p.traceBuffer = NewBatchBuffer(cfg.BatchSize, cfg.FlushInterval, p.flushTraces)
	p.metricBuffer = NewBatchBuffer(cfg.BatchSize, cfg.FlushInterval, p.flushMetrics)
	p.logBuffer = NewBatchBuffer(cfg.BatchSize, cfg.FlushInterval, p.flushLogs)

	return p
}

func (p *IngestionPipeline) IngestTrace(record TraceRecord) {
	p.traceBuffer.Add(record)
}

func (p *IngestionPipeline) IngestMetric(record MetricRecord) {
	p.metricBuffer.Add(record)
}

func (p *IngestionPipeline) IngestLog(record LogRecord) {
	p.logBuffer.Add(record)
}

func (p *IngestionPipeline) flushTraces(records []TraceRecord) {
	if len(records) == 0 {
		return
	}
	jsonData, err := json.Marshal(records)
	if err != nil {
		fmt.Printf("error marshaling traces: %v\n", err)
		return
	}
	p.sendBatch("scope_traces", jsonData)
}

func (p *IngestionPipeline) flushMetrics(records []MetricRecord) {
	if len(records) == 0 {
		return
	}
	jsonData, err := json.Marshal(records)
	if err != nil {
		fmt.Printf("error marshaling metrics: %v\n", err)
		return
	}
	p.sendBatch("scope_metrics", jsonData)
}

func (p *IngestionPipeline) flushLogs(records []LogRecord) {
	if len(records) == 0 {
		return
	}
	jsonData, err := json.Marshal(records)
	if err != nil {
		fmt.Printf("error marshaling logs: %v\n", err)
		return
	}
	p.sendBatch("scope_logs", jsonData)
}

func (p *IngestionPipeline) sendBatch(table string, jsonData []byte) {
	query := fmt.Sprintf("INSERT INTO %s FORMAT JSONEachRow", table)
	url := fmt.Sprintf("%s/?query=%s", p.clickhouseURL, query)

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(jsonData))
	if err != nil {
		fmt.Printf("error creating request for %s: %v\n", table, err)
		return
	}
	req.Header.Set("Content-Type", "application/json")

	resp, err := p.httpClient.Do(req)
	if err != nil {
		fmt.Printf("error sending batch to %s: %v\n", table, err)
		return
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(resp.Body)
		fmt.Printf("clickhouse rejected batch for %s (status %d): %s\n", table, resp.StatusCode, string(body))
	}
}

func (p *IngestionPipeline) FlushAll() {
	p.traceBuffer.Flush()
	p.metricBuffer.Flush()
	p.logBuffer.Flush()
}

func (p *IngestionPipeline) Shutdown() {
	p.traceBuffer.Stop()
	p.metricBuffer.Stop()
	p.logBuffer.Stop()
	p.FlushAll()
}
