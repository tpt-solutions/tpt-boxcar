export interface ServiceNode {
  name: string;
  health: "healthy" | "degraded" | "down";
  type: string;
  dependencies: string[];
}

export interface TraceSpan {
  id: string;
  name: string;
  service: string;
  start: number;
  duration: number;
  parentId: string | null;
}

export interface MetricPoint {
  timestamp: number;
  value: number;
}

export interface MetricSeries {
  name: string;
  points: MetricPoint[];
}

export interface LogEntry {
  time: string;
  service: string;
  severity: string;
  message: string;
}

export interface WasmModule {
  name: string;
  compileTime: string;
  instantiateTime: string;
  memoryPages: number;
  memoryMB: string;
}
