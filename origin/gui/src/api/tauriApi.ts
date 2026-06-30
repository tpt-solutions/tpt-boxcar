import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface Service {
  id: string;
  name: string;
  kind: "oci" | "wasm";
  status: "running" | "stopped" | "error";
  cpu: number;
  memMb: number;
}

export type LogEvent = { service: string; line: string; timestamp: number };

export const listServices = () => invoke<Service[]>("list_services");
export const startService = (id: string) =>
  invoke<void>("start_service", { id });
export const stopService = (id: string) =>
  invoke<void>("stop_service", { id });
export const applyManifest = (yaml: string) =>
  invoke<void>("apply_manifest", { yaml });

export const streamLogs = (
  handler: (e: LogEvent) => void
): Promise<UnlistenFn> =>
  listen<LogEvent>("log-event", (event) => handler(event.payload));
