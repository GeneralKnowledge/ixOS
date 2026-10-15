#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

"$ROOT/scripts/generate-bindings.sh" "$TMP/out"

for file in forge_ffi.swift forge_ffiFFI.h forge_ffiFFI.modulemap; do
  if ! diff -u "$ROOT/examples/counter/Generated/$file" "$TMP/out/$file"; then
    echo "error: $file is stale; run scripts/generate-bindings.sh" >&2
    exit 1
  fi
done

echo "[forge] Generated bindings match committed copies."
