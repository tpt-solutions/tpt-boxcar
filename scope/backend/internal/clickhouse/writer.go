package clickhouse

import (
	"context"
	"fmt"
	"time"

	"github.com/ClickHouse/clickhouse-go/v2"
	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"

	"github.com/tpt-cloud-native/scope/backend/internal/schema"
)

// Writer writes internal schema records to ClickHouse via native protocol.
type Writer struct {
	conn driver.Conn
}

// Options configures the ClickHouse connection.
type Options struct {
	Addr     string
	Database string
	Username string
	Password string
}

// NewWriter opens a native-protocol connection to ClickHouse.
func NewWriter(ctx context.Context, opts Options) (*Writer, error) {
	conn, err := clickhouse.Open(&clickhouse.Options{
		Addr: []string{opts.Addr},
		Auth: clickhouse.Auth{
			Database: opts.Database,
			Username: opts.Username,
			Password: opts.Password,
		},
		DialTimeout:     5 * time.Second,
		MaxOpenConns:    10,
		MaxIdleConns:    5,
		ConnMaxLifetime: time.Hour,
	})
	if err != nil {
		return nil, fmt.Errorf("open clickhouse: %w", err)
	}
	if err := conn.Ping(ctx); err != nil {
		conn.Close()
		return nil, fmt.Errorf("ping clickhouse: %w", err)
	}
	return &Writer{conn: conn}, nil
}

// EnsureTables creates the schema tables if they do not exist.
func (w *Writer) EnsureTables(ctx context.Context) error {
	for _, ddl := range []string{
		schema.CreateTracesTable,
		schema.CreateMetricsTable,
		schema.CreateLogsTable,
		schema.CreateRawEventsTable,
	} {
		if err := w.conn.Exec(ctx, ddl); err != nil {
			return fmt.Errorf("exec DDL: %w", err)
		}
	}
	return nil
}

// WriteTraces inserts a batch of TraceRecords using the native batch protocol.
func (w *Writer) WriteTraces(ctx context.Context, records []schema.TraceRecord) error {
	if len(records) == 0 {
		return nil
	}
	batch, err := w.conn.PrepareBatch(ctx, "INSERT INTO scope_traces")
	if err != nil {
		return fmt.Errorf("prepare traces batch: %w", err)
	}
	for _, r := range records {
		if err := batch.Append(
			r.TraceID, r.SpanID, r.ParentSpanID, r.ServiceName, r.OperationName,
			r.StartTime, r.EndTime, r.DurationNs,
			r.Status, r.StatusMessage, r.SpanKind,
			r.ContainerID, r.ImageName, r.PodName, r.Namespace, r.NodeName,
			r.Attributes, r.Events,
		); err != nil {
			return fmt.Errorf("append trace: %w", err)
		}
	}
	return batch.Send()
}

// WriteMetrics inserts a batch of MetricRecords using the native batch protocol.
func (w *Writer) WriteMetrics(ctx context.Context, records []schema.MetricRecord) error {
	if len(records) == 0 {
		return nil
	}
	batch, err := w.conn.PrepareBatch(ctx, "INSERT INTO scope_metrics")
	if err != nil {
		return fmt.Errorf("prepare metrics batch: %w", err)
	}
	for _, r := range records {
		if err := batch.Append(
			r.MetricName, r.MetricType, r.Timestamp, r.Value, r.Labels,
			r.ServiceName, r.ContainerID, r.ImageName, r.PodName, r.Namespace, r.NodeName,
		); err != nil {
			return fmt.Errorf("append metric: %w", err)
		}
	}
	return batch.Send()
}

// WriteLogs inserts a batch of LogRecords using the native batch protocol.
func (w *Writer) WriteLogs(ctx context.Context, records []schema.LogRecord) error {
	if len(records) == 0 {
		return nil
	}
	batch, err := w.conn.PrepareBatch(ctx, "INSERT INTO scope_logs")
	if err != nil {
		return fmt.Errorf("prepare logs batch: %w", err)
	}
	for _, r := range records {
		if err := batch.Append(
			r.Timestamp, r.Level, r.ServiceName, r.Message,
			r.TraceID, r.SpanID,
			r.ContainerID, r.ImageName, r.PodName, r.Namespace, r.NodeName,
			r.Attributes,
		); err != nil {
			return fmt.Errorf("append log: %w", err)
		}
	}
	return batch.Send()
}

// Close shuts down the connection.
func (w *Writer) Close() error {
	return w.conn.Close()
}
