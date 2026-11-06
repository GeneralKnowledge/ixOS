#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
# shellcheck source=forge-crate.sh
source "$ROOT/scripts/forge-crate.sh"

GENERATED="${1:-${FORGE_GENERATED_DIR:-examples/counter/Generated}}"
"$ROOT/scripts/generate-bindings.sh" "$GENERATED"

IOS_LIB="$NATIVE_OUT/ios-device/lib/libforge_ffi.a"
if "$ROOT/scripts/native-lib-needs-rebuild.sh" "$IOS_LIB" ios; then
  "$ROOT/scripts/build-rust-ios.sh"
fi

echo "[forge] iOS static library ready under ${NATIVE_OUT}/ios-device; Swift bindings in ${GENERATED}."
