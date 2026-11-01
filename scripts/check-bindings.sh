#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

"$ROOT/scripts/generate-bindings.sh" "$TMP/swift"

for file in ForgeFFI/forge_ffi.swift forge_ffiFFI/forge_ffiFFI.h forge_ffiFFI/module.modulemap; do
  if ! diff -u "$ROOT/examples/counter/Generated/$file" "$TMP/swift/$file"; then
    echo "error: $file is stale; run scripts/generate-bindings.sh" >&2
    exit 1
  fi
done

"$ROOT/scripts/generate-bindings.sh" "$TMP/kotlin" kotlin

KOTLIN_FILE="uniffi/forge_ffi/forge_ffi.kt"
if ! diff -u "$ROOT/examples/counter-android/Generated/$KOTLIN_FILE" "$TMP/kotlin/$KOTLIN_FILE"; then
  echo "error: $KOTLIN_FILE is stale; run scripts/generate-bindings.sh examples/counter-android/Generated kotlin" >&2
  exit 1
fi

echo "[forge] Generated bindings match committed copies."
