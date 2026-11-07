#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

"$ROOT/scripts/generate-bindings.sh" "$TMP/swift"

# Every iOS example on the shared forge-ffi carries its own copy of the Swift
# bindings. Chat and Pocket Library own their crates: check-app-crates.sh.
for example in counter weather; do
  for file in ForgeFFI/forge_ffi.swift forge_ffiFFI/forge_ffiFFI.h forge_ffiFFI/module.modulemap; do
    if ! diff -u "$ROOT/examples/$example/Generated/$file" "$TMP/swift/$file"; then
      echo "error: examples/$example/Generated/$file is stale; run scripts/generate-bindings.sh examples/$example/Generated" >&2
      exit 1
    fi
  done
done

"$ROOT/scripts/generate-bindings.sh" "$TMP/kotlin" kotlin

# Every Android example carries its own copy of the Kotlin bindings.
for example in counter-android chat-android; do
  for KOTLIN_FILE in uniffi/forge_ffi/forge_ffi.kt org/forgemobile/ffi/ForgeAndroid.kt; do
    if ! diff -u "$ROOT/examples/$example/Generated/$KOTLIN_FILE" "$TMP/kotlin/$KOTLIN_FILE"; then
      echo "error: examples/$example/Generated/$KOTLIN_FILE is stale; run scripts/generate-bindings.sh examples/$example/Generated kotlin" >&2
      exit 1
    fi
  done
done

echo "[forge] Generated bindings match committed copies."
