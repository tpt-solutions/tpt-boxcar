package schema

const CreateTracesTable = `
CREATE TABLE IF NOT EXISTS scope_traces (
    trace_id       String,
    span_id        String,
    parent_span_id String,
    service_name   String,
    operation_name String,
    start_time     DateTime64(9, 'UTC'),
    end_time       DateTime64(9, 'UTC'),
    duration_ns    UInt64,
    status         LowCardinality(String),
    status_message String,
    span_kind      LowCardinality(String),
    container_id   String,
    image_name     String,
    pod_name       String,
    namespace      String,
    node_name      String,
    attributes     Map(String, String),
    events         String,
    INDEX idx_trace_id trace_id TYPE bloom_filter GRANULARITY 4,
    INDEX idx_service service_name TYPE set(100) GRANULARITY 4,
    INDEX idx_pod pod_name TYPE set(1000) GRANULARITY 4
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(start_time)
ORDER BY (service_name, start_time, trace_id)
TTL start_time + INTERVAL 30 DAY
SETTINGS index_granularity = 8192`

const CreateMetricsTable = `
CREATE TABLE IF NOT EXISTS scope_metrics (
    metric_name    LowCardinality(String),
    metric_type    LowCardinality(String),
    timestamp      DateTime64(9, 'UTC'),
    value          Float64,
    labels         Map(String, String),
    service_name   String,
    container_id   String,
    image_name     String,
    pod_name       String,
    namespace      String,
    node_name      String,
    INDEX idx_metric metric_name TYPE bloom_filter GRANULARITY 4,
    INDEX idx_service service_name TYPE set(100) GRANULARITY 4
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(timestamp)
ORDER BY (metric_name, service_name, timestamp)
TTL timestamp + INTERVAL 30 DAY
SETTINGS index_granularity = 8192`

const CreateLogsTable = `
CREATE TABLE IF NOT EXISTS scope_logs (
    timestamp      DateTime64(9, 'UTC'),
    level          LowCardinality(String),
    service_name   String,
    message        String,
    trace_id       String,
    span_id        String,
    container_id   String,
    image_name     String,
    pod_name       String,
    namespace      String,
    node_name      String,
    attributes     Map(String, String),
    INDEX idx_level level TYPE set(5) GRANULARITY 4,
    INDEX idx_service service_name TYPE set(100) GRANULARITY 4,
    INDEX idx_trace trace_id TYPE bloom_filter GRANULARITY 4
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(timestamp)
ORDER BY (service_name, timestamp)
TTL timestamp + INTERVAL 30 DAY
SETTINGS index_granularity = 8192`

const CreateRawEventsTable = `
CREATE TABLE IF NOT EXISTS scope_raw_events (
    timestamp     DateTime64(9, 'UTC'),
    event_type    LowCardinality(String),
    pid           UInt32,
    tid           UInt32,
    comm          String,
    container_id  String,
    image_name    String,
    pod_name      String,
    namespace     String,
    node_name     String,
    data          String,
    INDEX idx_event event_type TYPE set(10) GRANULARITY 4
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(timestamp)
ORDER BY (event_type, timestamp)
TTL timestamp + INTERVAL 7 DAY
SETTINGS index_granularity = 8192`
