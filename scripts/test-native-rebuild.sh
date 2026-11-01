#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

FAKE="$TMP/libforge_ffi.a"
touch "$FAKE"
"$ROOT/scripts/write-native-build-meta.sh" "$FAKE" ios

if "$ROOT/scripts/native-lib-needs-rebuild.sh" "$FAKE" ios; then
  echo "error: expected fresh artifact to skip rebuild" >&2
  exit 1
fi

LIBRS="$ROOT/crates/forge-ffi/src/lib.rs"
# Remember the source mtime so the probe below only bumps it temporarily; the
# file contents are never modified.
touch -r "$LIBRS" "$TMP/librs-mtime"
trap 'touch -r "$TMP/librs-mtime" "$LIBRS" 2>/dev/null || true; rm -rf "$TMP"' EXIT

sleep 1
touch "$LIBRS"

if ! "$ROOT/scripts/native-lib-needs-rebuild.sh" "$FAKE" ios; then
  echo "error: expected stale detection after source touch" >&2
  exit 1
fi

touch -r "$TMP/librs-mtime" "$LIBRS"

touch "$TMP/missing.so"
if ! "$ROOT/scripts/native-lib-needs-rebuild.sh" "$TMP/missing.so" android; then
  echo "error: missing artifact should require rebuild" >&2
  exit 1
fi

echo "[forge] native-lib-needs-rebuild.sh checks passed."
