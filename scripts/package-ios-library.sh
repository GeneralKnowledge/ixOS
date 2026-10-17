#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

GENERATED="${1:-${FORGE_GENERATED_DIR:-examples/counter/Generated}}"
"$ROOT/scripts/generate-bindings.sh" "$GENERATED"

IOS_LIB="$ROOT/build/ios-device/lib/libforge_ffi.a"
if "$ROOT/scripts/native-lib-needs-rebuild.sh" "$IOS_LIB" ios; then
  "$ROOT/scripts/build-rust-ios.sh"
fi

echo "[forge] iOS static library ready under build/ios-device; Swift bindings in ${GENERATED}."
