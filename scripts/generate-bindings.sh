#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

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

echo "[forge] Building forge-ffi (host) for binding generation..."
cargo build -p forge-ffi

case "$(uname -s)" in
  Darwin) LIB_EXT="dylib" ;;
  *) LIB_EXT="so" ;;
esac
LIB="$ROOT/target/debug/libforge_ffi.${LIB_EXT}"
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
fi

echo "[forge] Bindings generated."
