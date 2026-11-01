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
cargo run -p uniffi-bindgen -- generate \
  --library "$LIB" \
  --language "$LANGUAGE" \
  --out-dir "$OUT"

echo "[forge] Bindings generated."
