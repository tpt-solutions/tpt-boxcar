#!/usr/bin/env bash
# Phase 10 Slice 6: multi-arch-by-default verification.
#
# Runs a fixture .wasm module through `wasmtime` (or `tpt origin replay`, if
# a manifest/scope-url is supplied) on the current host and prints the CPU
# architecture plus wall-clock timing, so the same script can be executed
# manually on both an ARM64 and an x86_64 machine to gather the numbers for
# docs/docs/phase10/multiarch-benchmark.md. Not part of CI — CI runners are
# typically single-arch, and this is deliberately a manual cross-arch check.
set -euo pipefail

WASM_FIXTURE="${1:-}"
if [[ -z "$WASM_FIXTURE" ]]; then
    echo "Usage: $0 <path-to-fixture.wasm>" >&2
    echo "  e.g. $0 origin/core/tests/fixtures/hello.wasm" >&2
    exit 1
fi

if ! command -v wasmtime >/dev/null 2>&1; then
    echo "error: wasmtime CLI not found on PATH (install: https://wasmtime.dev)" >&2
    exit 1
fi

ARCH="$(uname -m)"
OS="$(uname -s)"

echo "host: ${OS} / ${ARCH}"
echo "fixture: ${WASM_FIXTURE} ($(sha256sum "$WASM_FIXTURE" | awk '{print $1}'))"

START_NS=$(date +%s%N)
OUTPUT="$(wasmtime run "$WASM_FIXTURE" 2>&1)"
END_NS=$(date +%s%N)

ELAPSED_MS=$(( (END_NS - START_NS) / 1000000 ))

echo "output: ${OUTPUT}"
echo "elapsed_ms: ${ELAPSED_MS}"
echo
echo "Record this (arch, elapsed_ms, output) in docs/docs/phase10/multiarch-benchmark.md"
echo "alongside a run from a different architecture, then diff the two 'output' lines"
echo "for byte-for-byte equality."
