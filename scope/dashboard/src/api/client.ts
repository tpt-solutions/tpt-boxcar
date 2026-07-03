import type { ServiceNode, TraceSpan, MetricSeries, LogEntry, WasmModule } from "./types";

const BASE_URL = import.meta.env.VITE_API_BASE_URL ?? '';

async function fetchJson<T>(path: string): Promise<T> {
  const res = await fetch(`${BASE_URL}${path}`);
  if (!res.ok) throw new Error(`API error ${res.status}: ${await res.text()}`);
  return res.json();
}

export interface QueryFilter {
  /** Go duration string, e.g. "15m", "1h" (default: backend's own default, currently 1h). */
  since?: string;
  service?: string;
}

function withQuery(path: string, filter?: QueryFilter): string {
  if (!filter) return path;
  const params = new URLSearchParams();
  if (filter.since) params.set("since", filter.since);
  if (filter.service) params.set("service", filter.service);
  const qs = params.toString();
  return qs ? `${path}?${qs}` : path;
}

export function getServices(): Promise<ServiceNode[]> {
  return fetchJson("/api/v1/services");
}

export function getTraces(filter?: QueryFilter): Promise<TraceSpan[]> {
  return fetchJson(withQuery("/api/v1/traces", filter));
}

export function getMetrics(filter?: QueryFilter): Promise<MetricSeries[]> {
  return fetchJson(withQuery("/api/v1/metrics", filter));
}

export function getLogs(filter?: QueryFilter): Promise<LogEntry[]> {
  return fetchJson(withQuery("/api/v1/logs", filter));
}

export function getWasmMetrics(): Promise<WasmModule[]> {
  return fetchJson("/api/v1/wasm");
}

/**
 * Open a Server-Sent Events connection to the real-time log tail endpoint.
 * Returns a cleanup function — call it to close the stream.
 *
 * Falls back silently if the browser doesn't support EventSource (never in practice).
 */
export function streamLogs(onEntry: (entry: LogEntry) => void): () => void {
  const es = new EventSource(`${BASE_URL}/api/v1/logs/stream`);
  es.onmessage = (e) => {
    try {
      const entry: LogEntry = JSON.parse(e.data);
      onEntry(entry);
    } catch {
      // ignore malformed frames
    }
  };
  return () => es.close();
}
