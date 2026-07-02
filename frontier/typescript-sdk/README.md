# @tpt-boxcar/frontier-plugin-sdk

TypeScript SDK for building TPT Frontier Wasm plugins.

## Installation

```bash
npm install @tpt-boxcar/frontier-plugin-sdk
```

## Quick Start

```typescript
import {
  FrontierPlugin,
  HostFunctions,
  HttpRequest,
  HttpResponse,
  FilterResult,
  PluginContext,
  continueResult,
  denyResult,
} from "@tpt-boxcar/frontier-plugin-sdk";

const plugin: FrontierPlugin = {
  init(host: HostFunctions) {
    host.log(1, "plugin initialized");
  },

  onRequest(request: HttpRequest, ctx: PluginContext): FilterResult {
    const auth = request.headers["authorization"];
    if (!auth) {
      return denyResult("missing authorization header");
    }
    return continueResult();
  },

  onResponse(
    request: HttpRequest,
    response: HttpResponse,
    ctx: PluginContext
  ): FilterResult {
    return continueResult();
  },

  shutdown() {},
};

export default plugin;
```

## Plugin Interface

Every plugin must implement `FrontierPlugin`:

- `init(host)` — Called once when the plugin is loaded. Store the `HostFunctions` reference.
- `onRequest(request, ctx)` — Called before the request is forwarded upstream.
- `onResponse(request, response, ctx)` — Called after the upstream response is received.
- `shutdown()` — Called when the plugin is unloaded.

## Filter Results

- `continueResult()` — Pass the request/response through unmodified.
- `modifyRequest(request)` — Modify the request before it is sent upstream.
- `modifyResponse(response)` — Modify the response before it is returned to the client.
- `denyResult(reason)` — Reject the request with a reason.

## Host Functions

The `HostFunctions` interface provides:

- `log(level, message)` — Write to the proxy's log stream.
- `getSharedData(key)` / `setSharedData(key, value)` — Shared key-value store across plugins.
- `getConfig(key)` — Read plugin configuration.
- `httpRequest(request)` — Make an outbound HTTP request from the proxy.

## ABI Compatibility

This SDK targets ABI version 1, matching the Rust plugin SDK (`tpt-frontier-plugin-sdk`).
