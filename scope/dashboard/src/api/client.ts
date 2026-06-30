import type { ServiceNode, TraceSpan, MetricSeries, LogEntry, WasmModule } from "./types";

const BASE_URL = import.meta.env.VITE_API_BASE_URL ?? '';

async function fetchJson<T>(path: string): Promise<T> {
  const res = await fetch(`${BASE_URL}${path}`);
  if (!res.ok) throw new Error(`API error ${res.status}: ${await res.text()}`);
  return res.json();
}

export function getServices(): Promise<ServiceNode[]> {
  return fetchJson("/api/v1/services");
}

export function getTraces(): Promise<TraceSpan[]> {
  return fetchJson("/api/v1/traces");
}

export function getMetrics(): Promise<MetricSeries[]> {
  return fetchJson("/api/v1/metrics");
}

export function getLogs(): Promise<LogEntry[]> {
  return fetchJson("/api/v1/logs");
}

export function getWasmMetrics(): Promise<WasmModule[]> {
  return fetchJson("/api/v1/wasm");
}
