#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
# shellcheck source=forge-crate.sh
source "$ROOT/scripts/forge-crate.sh"

OUT="${1:-examples/counter/Generated}"
LANGUAGE="${2:-swift}"

case "$LANGUAGE" in
  swift | kotlin) ;;
  *)
    echo "error: unsupported language '$LANGUAGE' (use swift or kotlin)" >&2
    exit 1
    ;;
esac

mkdir -p "$OUT"

echo "[forge] Building ${CRATE_DIR#"$ROOT/"} (host) for binding generation..."
cargo build "${CARGO_SELECT[@]}"

case "$(uname -s)" in
  Darwin) LIB_EXT="dylib" ;;
  *) LIB_EXT="so" ;;
esac
LIB="$CARGO_TARGET/debug/libforge_ffi.${LIB_EXT}"
if [[ ! -f "$LIB" ]]; then
  echo "error: expected host library at $LIB" >&2
  exit 1
fi

echo "[forge] Generating ${LANGUAGE} bindings into $OUT..."
if [[ "$LANGUAGE" == "kotlin" ]]; then
  cargo run -p uniffi-bindgen -- generate \
    --library "$LIB" \
    --language kotlin \
    --out-dir "$OUT"
  # Hand-written Android glue that travels with the bindings (ForgeAndroid.init:
  # the app Context for the HTTPS certificate verifier). Source of truth:
  # crates/forge-runtime/kotlin/.
  mkdir -p "$OUT/org/forgemobile/ffi"
  cp "$ROOT/crates/forge-runtime/kotlin/org/forgemobile/ffi/ForgeAndroid.kt" "$OUT/org/forgemobile/ffi/ForgeAndroid.kt"
else
  # SwiftPM needs the Swift source and the C module in separate folders:
  #   Generated/ForgeFFI/forge_ffi.swift                  (Swift target "ForgeFFI")
  #   Generated/forge_ffiFFI/{forge_ffiFFI.h,module.modulemap}  (system library "forge_ffiFFI")
  STAGE="$(mktemp -d)"
  trap 'rm -rf "$STAGE"' EXIT
  cargo run -p uniffi-bindgen -- generate \
    --library "$LIB" \
    --language swift \
    --out-dir "$STAGE"
  mkdir -p "$OUT/ForgeFFI" "$OUT/forge_ffiFFI"
  mv "$STAGE/forge_ffi.swift" "$OUT/ForgeFFI/forge_ffi.swift"
  mv "$STAGE/forge_ffiFFI.h" "$OUT/forge_ffiFFI/forge_ffiFFI.h"
  mv "$STAGE/forge_ffiFFI.modulemap" "$OUT/forge_ffiFFI/module.modulemap"
  # Older layout kept everything at the top level, which SwiftPM rejects.
  rm -f "$OUT/forge_ffi.swift" "$OUT/forge_ffiFFI.h" "$OUT/forge_ffiFFI.modulemap"
  # Native services: when the crate uses `forge_services::native_services!()`,
  # Forge's Swift adapters (Keychain, notifications, ...) compile in the same
  # module as the bindings, so they can implement its protocols. Source of
  # truth: crates/forge-services/swift/. Crates without the macro get none.
  rm -f "$OUT/ForgeFFI"/ForgeServices*.swift
  if grep -q 'public func installNativeServices(' "$OUT/ForgeFFI/forge_ffi.swift"; then
    cp "$ROOT/crates/forge-services/swift/"ForgeServices*.swift "$OUT/ForgeFFI/"
    echo "[forge] Added the native services Swift adapters."
  fi
fi

echo "[forge] Bindings generated."
