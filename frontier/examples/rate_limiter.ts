import {
  FrontierPlugin,
  HostFunctions,
  HttpRequest,
  HttpResponse,
  FilterResult,
  PluginContext,
  continueResult,
  denyResult,
} from "@tpt-cloud-native/frontier-plugin-sdk";

interface RateLimitEntry {
  count: number;
  windowStart: number;
}

class RateLimiterPlugin implements FrontierPlugin {
  private host: HostFunctions | null = null;
  private maxRequests: number = 100;
  private windowMs: number = 60_000;
  private counters: Map<string, RateLimitEntry> = new Map();

  init(host: HostFunctions): void {
    this.host = host;

    const maxStr = host.getConfig("max_requests");
    if (maxStr) this.maxRequests = parseInt(maxStr, 10);

    const windowStr = host.getConfig("window_duration_ms");
    if (windowStr) this.windowMs = parseInt(windowStr, 10);

    host.log(1, `rate limiter initialized: max=${this.maxRequests}, window=${this.windowMs}ms`);
  }

  onRequest(request: HttpRequest, ctx: PluginContext): FilterResult {
    const clientIp = request.headers["x-forwarded-for"] || request.headers["x-real-ip"] || "unknown";
    const now = Date.now();
    const entry = this.counters.get(clientIp);

    if (!entry || now - entry.windowStart > this.windowMs) {
      this.counters.set(clientIp, { count: 1, windowStart: now });
      return continueResult();
    }

    entry.count++;

    if (entry.count > this.maxRequests) {
      const retryAfter = Math.ceil((entry.windowStart + this.windowMs - now) / 1000);
      this.host?.log(2, `rate limit exceeded for ${clientIp}`);
      return denyResult(`rate limit exceeded, retry after ${retryAfter}s`);
    }

    return continueResult();
  }

  onResponse(request: HttpRequest, response: HttpResponse, ctx: PluginContext): FilterResult {
    const clientIp = request.headers["x-forwarded-for"] || request.headers["x-real-ip"] || "unknown";
    const entry = this.counters.get(clientIp);

    if (entry) {
      response.headers["x-ratelimit-remaining"] = String(Math.max(0, this.maxRequests - entry.count));
      response.headers["x-ratelimit-limit"] = String(this.maxRequests);
    }

    return continueResult();
  }

  shutdown(): void {
    this.counters.clear();
    this.host?.log(1, "rate limiter shut down");
  }
}

const plugin = new RateLimiterPlugin();
export default plugin;
