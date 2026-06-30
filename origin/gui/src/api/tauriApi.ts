import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface Service {
  name: string;
  type: "oci" | "wasm";
  status: "running" | "stopped" | "starting";
  cpu: string;
  ram: string;
}

export interface LogEntry {
  timestamp: string;
  message: string;
  service: string;
}

export async function listServices(): Promise<Service[]> {
  return invoke<Service[]>("list_services");
}

export async function startService(name: string): Promise<void> {
  return invoke("start_service", { name });
}

export async function stopService(name: string): Promise<void> {
  return invoke("stop_service", { name });
}

export function streamLogs(
  callback: (entry: LogEntry) => void
): Promise<UnlistenFn> {
  return listen<LogEntry>("log-entry", (event) => {
    callback(event.payload);
  });
}

export async function applyManifest(content: string): Promise<void> {
  return invoke("apply_manifest", { content });
}
