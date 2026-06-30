use std::collections::HashMap;
use std::time::Duration;

use opentelemetry::global;
use opentelemetry::metrics::{Counter, Histogram, MeterProvider as _, UpDownCounter};
use opentelemetry::trace::{Span, SpanKind, Status, Tracer, TracerProvider};
use opentelemetry::KeyValue;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::runtime;
use opentelemetry_sdk::Resource;

use crate::enrichment::EnrichmentData;
use crate::probes::{EventData, ProbeEvent};

#[derive(Debug, Clone)]
pub struct OtelConfig {
    pub endpoint: String,
    pub service_name: String,
    pub batch_timeout: Duration,
    pub max_queue_size: usize,
    pub max_export_batch_size: usize,
}

impl Default for OtelConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:4317".to_string(),
            service_name: "tpt-scope-agent".to_string(),
            batch_timeout: Duration::from_secs(5),
            max_queue_size: 2048,
            max_export_batch_size: 512,
        }
    }
}

pub struct OtelExporter {
    tracer: opentelemetry_sdk::trace::Tracer,
    http_metrics: HttpMetrics,
    wasm_metrics: WasmMetrics,
    syscall_metrics: SyscallMetrics,
}

pub struct HttpMetrics {
    pub requests_total: Counter<u64>,
    pub request_duration: Histogram<u64>,
    pub bytes_sent: Counter<u64>,
    pub bytes_received: Counter<u64>,
    pub connections_active: UpDownCounter<i64>,
}

pub struct WasmMetrics {
    pub compile_duration: Histogram<u64>,
    pub instantiate_duration: Histogram<u64>,
    pub memory_pages: Histogram<u64>,
    pub instance_count: UpDownCounter<i64>,
}

pub struct SyscallMetrics {
    pub syscall_count: Counter<u64>,
    pub syscall_duration: Histogram<u64>,
    pub open_count: Counter<u64>,
    pub read_bytes: Counter<u64>,
    pub write_bytes: Counter<u64>,
}

impl OtelExporter {
    pub fn new(config: &OtelConfig) -> anyhow::Result<Self> {
        let resource = Resource::new(vec![
            KeyValue::new("service.name", config.service_name.clone()),
        ]);

        let tracer_provider = opentelemetry_otlp::new_pipeline()
            .tracing()
            .with_exporter(
                opentelemetry_otlp::new_exporter()
                    .tonic()
                    .with_endpoint(&config.endpoint),
            )
            .with_trace_config(
                opentelemetry_sdk::trace::Config::default().with_resource(resource.clone()),
            )
            .install_batch(runtime::Tokio)?;

        let meter_provider = opentelemetry_otlp::new_pipeline()
            .metrics(runtime::Tokio)
            .with_exporter(
                opentelemetry_otlp::new_exporter()
                    .tonic()
                    .with_endpoint(&config.endpoint),
            )
            .with_resource(resource)
            .build()?;

        let tracer = tracer_provider.tracer("tpt-scope-agent");
        let meter = meter_provider.meter("tpt-scope-agent");

        global::set_tracer_provider(tracer_provider);
        global::set_meter_provider(meter_provider);

        let http_metrics = HttpMetrics {
            requests_total: meter.u64_counter("http.requests.total").init(),
            request_duration: meter.u64_histogram("http.request.duration").init(),
            bytes_sent: meter.u64_counter("http.bytes.sent").init(),
            bytes_received: meter.u64_counter("http.bytes.received").init(),
            connections_active: meter.i64_up_down_counter("http.connections.active").init(),
        };

        let wasm_metrics = WasmMetrics {
            compile_duration: meter.u64_histogram("wasm.compile.duration").init(),
            instantiate_duration: meter.u64_histogram("wasm.instantiate.duration").init(),
            memory_pages: meter.u64_histogram("wasm.memory.pages").init(),
            instance_count: meter.i64_up_down_counter("wasm.instances").init(),
        };

        let syscall_metrics = SyscallMetrics {
            syscall_count: meter.u64_counter("syscall.count").init(),
            syscall_duration: meter.u64_histogram("syscall.duration").init(),
            open_count: meter.u64_counter("syscall.open.count").init(),
            read_bytes: meter.u64_counter("syscall.read.bytes").init(),
            write_bytes: meter.u64_counter("syscall.write.bytes").init(),
        };

        Ok(Self {
            tracer,
            http_metrics,
            wasm_metrics,
            syscall_metrics,
        })
    }

    pub fn emit_event(&self, event: &ProbeEvent, enrichment: Option<&EnrichmentData>) {
        match &event.data {
            EventData::Network(net_event) => {
                self.emit_network_trace(event, net_event, enrichment)
            }
            EventData::Syscall(sys_event) => {
                self.emit_syscall_trace(event, sys_event, enrichment)
            }
            EventData::WasmRuntime(wasm_event) => {
                self.emit_wasm_trace(event, wasm_event, enrichment)
            }
        }
    }

    fn emit_network_trace(
        &self,
        event: &ProbeEvent,
        net_event: &crate::probes::NetworkEvent,
        enrichment: Option<&EnrichmentData>,
    ) {
        let mut attributes = vec![
            KeyValue::new("src.addr", net_event.src_addr.to_string()),
            KeyValue::new("dst.addr", net_event.dst_addr.to_string()),
            KeyValue::new("src.port", net_event.src_port as i64),
            KeyValue::new("dst.port", net_event.dst_port as i64),
            KeyValue::new("event.type", format!("{:?}", net_event.event_type)),
            KeyValue::new("process.pid", event.pid as i64),
            KeyValue::new("process.comm", event.comm.clone()),
        ];

        if let Some(inc) = enrichment {
            attributes.push(KeyValue::new("container.id", inc.container_id.clone()));
            attributes.push(KeyValue::new("container.image", inc.image_name.clone()));
            attributes.push(KeyValue::new("k8s.pod.name", inc.pod_name.clone()));
            attributes.push(KeyValue::new("k8s.namespace", inc.namespace.clone()));
        }

        let mut span = self
            .tracer
            .span_builder(format!(
                "net.{}",
                format!("{:?}", net_event.event_type).to_lowercase()
            ))
            .with_kind(SpanKind::Internal)
            .with_attributes(attributes)
            .start(&self.tracer);

        if net_event.bytes > 0 {
            self.http_metrics.bytes_sent.add(net_event.bytes, &[]);
        }
        self.http_metrics.connections_active.add(1, &[]);

        span.set_status(Status::Ok);
        span.end();
    }

    fn emit_syscall_trace(
        &self,
        _event: &ProbeEvent,
        sys_event: &crate::probes::SyscallEvent,
        enrichment: Option<&EnrichmentData>,
    ) {
        let mut attributes = vec![
            KeyValue::new("syscall.nr", sys_event.syscall_nr as i64),
            KeyValue::new("event.type", format!("{:?}", sys_event.event_type)),
            KeyValue::new("process.pid", _event.pid as i64),
            KeyValue::new("process.comm", _event.comm.clone()),
        ];

        if let Some(ref path) = sys_event.path {
            attributes.push(KeyValue::new("file.path", path.clone()));
        }
        if let Some(fd) = sys_event.fd {
            attributes.push(KeyValue::new("file.fd", fd as i64));
        }
        if let Some(bytes) = sys_event.bytes_rw {
            attributes.push(KeyValue::new("io.bytes", bytes as i64));
        }

        if let Some(inc) = enrichment {
            attributes.push(KeyValue::new("container.id", inc.container_id.clone()));
            attributes.push(KeyValue::new("container.image", inc.image_name.clone()));
            attributes.push(KeyValue::new("k8s.pod.name", inc.pod_name.clone()));
            attributes.push(KeyValue::new("k8s.namespace", inc.namespace.clone()));
        }

        let mut span = self
            .tracer
            .span_builder(format!(
                "syscall.{}",
                format!("{:?}", sys_event.event_type).to_lowercase()
            ))
            .with_kind(SpanKind::Internal)
            .with_attributes(attributes)
            .start(&self.tracer);

        self.syscall_metrics.syscall_count.add(1, &[]);
        self.syscall_metrics
            .syscall_duration
            .record(sys_event.latency.as_millis() as u64, &[]);

        match sys_event.event_type {
            crate::probes::SyscallEventType::Open => {
                self.syscall_metrics.open_count.add(1, &[]);
            }
            crate::probes::SyscallEventType::Read => {
                if let Some(bytes) = sys_event.bytes_rw {
                    self.syscall_metrics.read_bytes.add(bytes, &[]);
                }
            }
            crate::probes::SyscallEventType::Write => {
                if let Some(bytes) = sys_event.bytes_rw {
                    self.syscall_metrics.write_bytes.add(bytes, &[]);
                }
            }
            _ => {}
        }

        span.set_status(Status::Ok);
        span.end();
    }

    fn emit_wasm_trace(
        &self,
        _event: &ProbeEvent,
        wasm_event: &crate::probes::WasmEvent,
        enrichment: Option<&EnrichmentData>,
    ) {
        let mut attributes = vec![
            KeyValue::new("wasm.module", wasm_event.module_name.clone()),
            KeyValue::new("wasm.instance.id", wasm_event.instance_id as i64),
            KeyValue::new("event.type", format!("{:?}", wasm_event.event_type)),
        ];

        if let Some(inc) = enrichment {
            attributes.push(KeyValue::new("container.id", inc.container_id.clone()));
            attributes.push(KeyValue::new("container.image", inc.image_name.clone()));
            attributes.push(KeyValue::new("k8s.pod.name", inc.pod_name.clone()));
            attributes.push(KeyValue::new("k8s.namespace", inc.namespace.clone()));
        }

        let mut span = self
            .tracer
            .span_builder(format!(
                "wasm.{}",
                format!("{:?}", wasm_event.event_type).to_lowercase()
            ))
            .with_kind(SpanKind::Internal)
            .with_attributes(attributes)
            .start(&self.tracer);

        let duration_ms = wasm_event.duration.as_millis() as u64;
        match wasm_event.event_type {
            crate::probes::WasmEventType::CompileStart
            | crate::probes::WasmEventType::CompileEnd => {
                self.wasm_metrics.compile_duration.record(duration_ms, &[]);
            }
            crate::probes::WasmEventType::Instantiate => {
                self.wasm_metrics
                    .instantiate_duration
                    .record(duration_ms, &[]);
                self.wasm_metrics.instance_count.add(1, &[]);
            }
            crate::probes::WasmEventType::MemoryGrow => {
                self.wasm_metrics.memory_pages.record(duration_ms, &[]);
            }
        }

        span.set_status(Status::Ok);
        span.end();
    }
}

pub struct BatchExporter {
    buffer: Vec<ProbeEvent>,
    enrichments: HashMap<String, EnrichmentData>,
    batch_size: usize,
}

impl BatchExporter {
    pub fn new(batch_size: usize, _flush_interval: Duration) -> Self {
        Self {
            buffer: Vec::with_capacity(batch_size),
            enrichments: HashMap::new(),
            batch_size,
        }
    }

    pub fn enqueue(&mut self, event: ProbeEvent) {
        self.buffer.push(event);
    }

    pub fn should_flush(&self) -> bool {
        self.buffer.len() >= self.batch_size
    }

    pub fn drain(&mut self) -> Vec<ProbeEvent> {
        std::mem::take(&mut self.buffer)
    }

    pub fn set_enrichment(&mut self, container_id: String, data: EnrichmentData) {
        self.enrichments.insert(container_id, data);
    }
}

pub fn build_span_attributes(
    event: &ProbeEvent,
    enrichment: Option<&EnrichmentData>,
) -> Vec<KeyValue> {
    let mut attrs = vec![
        KeyValue::new("process.pid", event.pid as i64),
        KeyValue::new("process.tid", event.tid as i64),
        KeyValue::new("process.comm", event.comm.clone()),
    ];

    if let Some(inc) = enrichment {
        attrs.push(KeyValue::new("container.id", inc.container_id.clone()));
        attrs.push(KeyValue::new("container.image", inc.image_name.clone()));
        attrs.push(KeyValue::new("k8s.pod.name", inc.pod_name.clone()));
        attrs.push(KeyValue::new("k8s.namespace", inc.namespace.clone()));
    }

    attrs
}

pub fn build_metric_resource(
    pod_name: &str,
    namespace: &str,
    container_id: &str,
) -> Resource {
    Resource::new(vec![
        KeyValue::new("k8s.pod.name", pod_name.to_string()),
        KeyValue::new("k8s.namespace", namespace.to_string()),
        KeyValue::new("container.id", container_id.to_string()),
    ])
}
