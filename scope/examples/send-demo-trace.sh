#!/usr/bin/env bash
# Sends one OTLP/HTTP JSON trace to a running `scope/backend/cmd/ingest` (localhost:4318)
# so you have something to query back out via `scope/backend/cmd/backend` (localhost:8081).
set -euo pipefail

NOW_NS=$(date +%s%N)
END_NS=$((NOW_NS + 42000000)) # +42ms

curl -sS -X POST http://localhost:4318/v1/traces \
  -H 'Content-Type: application/json' \
  -d @- <<JSON
{
  "resourceSpans": [{
    "resource": {
      "attributes": [{ "key": "service.name", "value": { "stringValue": "demo-service" } }]
    },
    "scopeSpans": [{
      "spans": [{
        "traceId": "5b8aa5a2d2c872e8321cf37308d69df2",
        "spanId": "051581bf3cb55c13",
        "name": "GET /hello",
        "kind": 2,
        "startTimeUnixNano": "${NOW_NS}",
        "endTimeUnixNano": "${END_NS}",
        "attributes": [{ "key": "http.method", "value": { "stringValue": "GET" } }]
      }]
    }]
  }]
}
JSON

echo "Sent. Query it back with: curl localhost:8081/api/v1/traces"
