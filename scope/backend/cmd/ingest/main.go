package main

import (
	"context"
	"flag"
	"fmt"
	"log"
	"net"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"google.golang.org/grpc"

	collectorlogs "go.opentelemetry.io/proto/otlp/collector/logs/v1"
	collectormetrics "go.opentelemetry.io/proto/otlp/collector/metrics/v1"
	collectortrace "go.opentelemetry.io/proto/otlp/collector/trace/v1"

	"github.com/tpt-cloud-native/scope/backend/internal/buffer"
	"github.com/tpt-cloud-native/scope/backend/internal/clickhouse"
	"github.com/tpt-cloud-native/scope/backend/internal/schema"
)

func main() {
	chAddr := flag.String("clickhouse-addr", "localhost:9000", "ClickHouse native protocol address")
	chDB := flag.String("clickhouse-db", "default", "ClickHouse database")
	chUser := flag.String("clickhouse-user", "default", "ClickHouse user")
	chPass := flag.String("clickhouse-pass", "", "Deprecated: use CLICKHOUSE_PASS env var instead")
	grpcPort := flag.Int("grpc-port", 4317, "OTLP gRPC receiver port")
	httpPort := flag.Int("http-port", 4318, "OTLP HTTP receiver port")
	metricsPort := flag.Int("metrics-port", 9090, "Prometheus metrics port")
	ringCap := flag.Int("ring-capacity", 100000, "Ring buffer capacity per signal type")
	flushInterval := flag.Duration("flush-interval", 5*time.Second, "Flush interval to ClickHouse")
	flag.Parse()

	// Resolve ClickHouse password: env var takes precedence over the deprecated CLI flag.
	password := os.Getenv("CLICKHOUSE_PASS")
	if password == "" {
		password = *chPass
	}
	if password == "" {
		log.Fatal("ClickHouse password must be provided via CLICKHOUSE_PASS env var (or deprecated -clickhouse-pass flag)")
	}

	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer cancel()

	// ClickHouse writer
	writer, err := clickhouse.NewWriter(ctx, clickhouse.Options{
		Addr:     *chAddr,
		Database: *chDB,
		Username: *chUser,
		Password: password,
	})
	if err != nil {
		log.Fatalf("connect clickhouse: %v", err)
	}
	defer writer.Close()

	if err := writer.EnsureTables(ctx); err != nil {
		log.Fatalf("ensure tables: %v", err)
	}
	log.Println("ClickHouse tables verified")

	// Ring buffers
	traceRing := buffer.NewRingBuffer[schema.TraceRecord](*ringCap)
	metricRing := buffer.NewRingBuffer[schema.MetricRecord](*ringCap)
	logRing := buffer.NewRingBuffer[schema.LogRecord](*ringCap)

	// Flushers
	traceFlusher := buffer.NewFlusher(traceRing, *flushInterval, func(records []schema.TraceRecord) {
		if err := writer.WriteTraces(context.Background(), records); err != nil {
			log.Printf("flush traces: %v", err)
		} else {
			log.Printf("flushed %d traces", len(records))
		}
	})
	metricFlusher := buffer.NewFlusher(metricRing, *flushInterval, func(records []schema.MetricRecord) {
		if err := writer.WriteMetrics(context.Background(), records); err != nil {
			log.Printf("flush metrics: %v", err)
		} else {
			log.Printf("flushed %d metrics", len(records))
		}
	})
	logFlusher := buffer.NewFlusher(logRing, *flushInterval, func(records []schema.LogRecord) {
		if err := writer.WriteLogs(context.Background(), records); err != nil {
			log.Printf("flush logs: %v", err)
		} else {
			log.Printf("flushed %d logs", len(records))
		}
	})

	traceFlusher.Start()
	metricFlusher.Start()
	logFlusher.Start()

	// gRPC server
	grpcSrv := grpc.NewServer()
	collectortrace.RegisterTraceServiceServer(grpcSrv, &traceServer{ring: traceRing})
	collectormetrics.RegisterMetricsServiceServer(grpcSrv, &metricsServer{ring: metricRing})
	collectorlogs.RegisterLogsServiceServer(grpcSrv, &logServer{ring: logRing})

	grpcLis, err := net.Listen("tcp", fmt.Sprintf(":%d", *grpcPort))
	if err != nil {
		log.Fatalf("grpc listen: %v", err)
	}
	go func() {
		log.Printf("OTLP gRPC receiver on :%d", *grpcPort)
		if err := grpcSrv.Serve(grpcLis); err != nil {
			log.Printf("grpc serve: %v", err)
		}
	}()

	// HTTP server
	hs := &httpServer{
		traceRing:  traceRing,
		metricRing: metricRing,
		logRing:    logRing,
	}
	httpMux := http.NewServeMux()
	httpMux.HandleFunc("/v1/traces", hs.HandleTraces)
	httpMux.HandleFunc("/v1/metrics", hs.HandleMetrics)
	httpMux.HandleFunc("/v1/logs", hs.HandleLogs)
	httpMux.Handle("/metrics", MetricsHandler())
	httpMux.HandleFunc("/health", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		fmt.Fprintf(w, `{"status":"healthy"}`)
	})

	httpSrv := &http.Server{
		Addr:    fmt.Sprintf(":%d", *httpPort),
		Handler: httpMux,
	}
	go func() {
		log.Printf("OTLP HTTP receiver on :%d", *httpPort)
		if err := httpSrv.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			log.Printf("http serve: %v", err)
		}
	}()

	// Prometheus metrics server
	metricsMux := http.NewServeMux()
	metricsMux.Handle("/metrics", MetricsHandler())
	metricsSrv := &http.Server{
		Addr:    fmt.Sprintf(":%d", *metricsPort),
		Handler: metricsMux,
	}
	go func() {
		log.Printf("Prometheus metrics on :%d", *metricsPort)
		if err := metricsSrv.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			log.Printf("metrics serve: %v", err)
		}
	}()

	<-ctx.Done()
	log.Println("shutting down...")

	grpcSrv.GracefulStop()
	httpSrv.Shutdown(context.Background())
	metricsSrv.Shutdown(context.Background())

	traceFlusher.Stop()
	metricFlusher.Stop()
	logFlusher.Stop()

	log.Println("shutdown complete")
}
