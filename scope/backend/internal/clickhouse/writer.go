package clickhouse

import (
	"context"
	"fmt"
	"time"

	"github.com/ClickHouse/clickhouse-go/v2"
	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"

	"github.com/tpt-boxcar/scope/backend/internal/schema"
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
		schema.CreateWasmInvocationsTable,
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

// WriteWasmInvocations inserts a batch of WasmInvocationRecords using the
// native batch protocol, for offline replay/time-travel debugging.
func (w *Writer) WriteWasmInvocations(ctx context.Context, records []schema.WasmInvocationRecord) error {
	if len(records) == 0 {
		return nil
	}
	batch, err := w.conn.PrepareBatch(ctx, "INSERT INTO scope_wasm_invocations")
	if err != nil {
		return fmt.Errorf("prepare wasm invocations batch: %w", err)
	}
	for _, r := range records {
		if err := batch.Append(
			r.Timestamp, r.ModuleName, r.Function, r.ArgsJSON, r.EnvJSON,
			r.WasmSha256, r.ServiceName, r.ContainerID,
		); err != nil {
			return fmt.Errorf("append wasm invocation: %w", err)
		}
	}
	return batch.Send()
}

// GetWasmInvocation fetches a single captured invocation by its row
// position within a module's history (id), for `origin replay` to consume
// offline. Returns nil if not found.
func (w *Writer) GetWasmInvocation(ctx context.Context, moduleName, id string) (*schema.WasmInvocationRecord, error) {
	row := w.conn.QueryRow(ctx, `
		SELECT timestamp, module_name, function, args_json, env_json, wasm_sha256, service_name, container_id
		FROM scope_wasm_invocations
		WHERE module_name = ? AND wasm_sha256 = ?
		ORDER BY timestamp DESC
		LIMIT 1`, moduleName, id)

	var r schema.WasmInvocationRecord
	if err := row.Scan(
		&r.Timestamp, &r.ModuleName, &r.Function, &r.ArgsJSON, &r.EnvJSON,
		&r.WasmSha256, &r.ServiceName, &r.ContainerID,
	); err != nil {
		return nil, fmt.Errorf("scan wasm invocation: %w", err)
	}
	return &r, nil
}

// Close shuts down the connection.
func (w *Writer) Close() error {
	return w.conn.Close()
}
