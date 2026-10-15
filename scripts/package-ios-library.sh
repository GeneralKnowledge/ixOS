#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

"$ROOT/scripts/generate-bindings.sh" "${FORGE_GENERATED_DIR:-examples/counter/Generated}"

if [[ ! -f "$ROOT/build/ios-device/lib/libforge_ffi.a" ]]; then
  "$ROOT/scripts/build-rust-ios.sh"
fi

echo "[forge] iOS static library and UniFFI artifacts are ready under build/ios-device and Generated/."
