export const ABI_VERSION = 1;

export interface PluginMetadata {
  name: string;
  version: string;
  author: string;
  description: string;
}

export interface HttpRequest {
  method: string;
  path: string;
  headers: Record<string, string>;
  body: Uint8Array;
}

export interface HttpResponse {
  status: number;
  headers: Record<string, string>;
  body: Uint8Array;
}

export enum FilterResultKind {
  Continue = 0,
  ModifyRequest = 1,
  ModifyResponse = 2,
  Deny = 3,
}

export type FilterResult =
  | { kind: FilterResultKind.Continue }
  | { kind: FilterResultKind.ModifyRequest; request: HttpRequest }
  | { kind: FilterResultKind.ModifyResponse; response: HttpResponse }
  | { kind: FilterResultKind.Deny; reason: string };

export interface PluginContext {
  request: HttpRequest;
  metadata: Record<string, string>;
}

export enum LogLevel {
  Debug = 0,
  Info = 1,
  Warn = 2,
  Error = 3,
}

export interface HostFunctions {
  log(level: LogLevel, message: string): void;
  getSharedData(key: string): Uint8Array | null;
  setSharedData(key: string, value: Uint8Array): void;
  getConfig(key: string): string | null;
  httpRequest(request: HttpRequest): HttpResponse;
}

export interface ConfigField {
  fieldType: string;
  required: boolean;
  default?: string;
  description: string;
}

export interface PluginManifest {
  abiVersion: number;
  metadata: PluginMetadata;
  permissions: string[];
  configSchema: Record<string, ConfigField>;
}

export interface FrontierPlugin {
  init(host: HostFunctions): void;
  onRequest(request: HttpRequest, ctx: PluginContext): FilterResult;
  onResponse(request: HttpRequest, response: HttpResponse, ctx: PluginContext): FilterResult;
  shutdown(): void;
}

export function continueResult(): FilterResult {
  return { kind: FilterResultKind.Continue };
}

export function modifyRequest(request: HttpRequest): FilterResult {
  return { kind: FilterResultKind.ModifyRequest, request };
}

export function modifyResponse(response: HttpResponse): FilterResult {
  return { kind: FilterResultKind.ModifyResponse, response };
}

export function denyResult(reason: string): FilterResult {
  return { kind: FilterResultKind.Deny, reason };
}

export function jsonResponse(data: unknown, status = 200): HttpResponse {
  const body = new TextEncoder().encode(JSON.stringify(data));
  return {
    status,
    headers: { "content-type": "application/json" },
    body,
  };
}

export function htmlResponse(html: string, status = 200): HttpResponse {
  const body = new TextEncoder().encode(html);
  return {
    status,
    headers: { "content-type": "text/html" },
    body,
  };
}
