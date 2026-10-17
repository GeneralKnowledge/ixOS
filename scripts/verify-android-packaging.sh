#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROJECT="${1:-$ROOT/examples/counter-android}"
JNI="$PROJECT/app/src/main/jniLibs"

for abi in arm64-v8a x86_64; do
  so="$JNI/$abi/libforge_ffi.so"
  if [[ ! -f "$so" ]]; then
    echo "error: missing $so (run package-android-library.sh)" >&2
    exit 1
  fi
done

KOTLIN="$PROJECT/Generated/uniffi/forge_ffi/forge_ffi.kt"
if [[ ! -f "$KOTLIN" ]]; then
  echo "error: missing generated Kotlin bindings at $KOTLIN" >&2
  exit 1
fi

echo "[forge] Android jniLibs and Kotlin bindings present for $PROJECT"
