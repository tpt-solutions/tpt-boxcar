import {
  FrontierPlugin,
  HostFunctions,
  HttpRequest,
  HttpResponse,
  FilterResult,
  PluginContext,
  continueResult,
  denyResult,
  jsonResponse,
} from "@tpt-cloud-native/frontier-plugin-sdk";

interface AuthConfig {
  apiKeyHeader: string;
  validApiKeys: Set<string>;
  bypassPaths: Set<string>;
}

class CustomAuthPlugin implements FrontierPlugin {
  private host: HostFunctions | null = null;
  private config: AuthConfig = {
    apiKeyHeader: "x-api-key",
    validApiKeys: new Set(),
    bypassPaths: new Set(["/health", "/ready", "/metrics"]),
  };

  init(host: HostFunctions): void {
    this.host = host;

    const header = host.getConfig("api_key_header");
    if (header) this.config.apiKeyHeader = header;

    const keysStr = host.getConfig("valid_api_keys");
    if (keysStr) {
      keysStr.split(",").forEach((k) => this.config.validApiKeys.add(k.trim()));
    }

    const bypassStr = host.getConfig("bypass_paths");
    if (bypassStr) {
      bypassStr.split(",").forEach((p) => this.config.bypassPaths.add(p.trim()));
    }

    host.log(1, `custom auth initialized: ${this.config.validApiKeys.size} keys, ${this.config.bypassPaths.size} bypass paths`);
  }

  onRequest(request: HttpRequest, ctx: PluginContext): FilterResult {
    if (this.config.bypassPaths.has(request.path)) {
      return continueResult();
    }

    const apiKey = request.headers[this.config.apiKeyHeader];

    if (!apiKey) {
      this.host?.log(2, `missing ${this.config.apiKeyHeader} header for ${request.path}`);
      return denyResult(`missing ${this.config.apiKeyHeader} header`);
    }

    if (!this.config.validApiKeys.has(apiKey)) {
      this.host?.log(2, `invalid API key for ${request.path}`);
      return denyResult("invalid API key");
    }

    this.host?.log(0, `authenticated request to ${request.path}`);
    return continueResult();
  }

  onResponse(request: HttpRequest, response: HttpResponse, ctx: PluginContext): FilterResult {
    return continueResult();
  }

  shutdown(): void {
    this.config.validApiKeys.clear();
    this.config.bypassPaths.clear();
    this.host?.log(1, "custom auth shut down");
  }
}

const plugin = new CustomAuthPlugin();
export default plugin;
