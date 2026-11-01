#!/usr/bin/env bash
# Runs a Swift host package's XCTest suite on macOS against a host build of forge-ffi.
# Usage: scripts/test-swift-host.sh [package-dir]   (default: examples/counter)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PACKAGE="${1:-examples/counter}"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "error: Swift host tests need macOS (SwiftUI and Combine)" >&2
  exit 1
fi

cd "$ROOT"
echo "[forge] Building forge-ffi for the macOS host..."
cargo build -p forge-ffi

# Only the static library goes in the search path, so the test binary does not
# need the .dylib at run time.
LIB_DIR="$ROOT/build/macos-host/lib"
mkdir -p "$LIB_DIR"
cp "$ROOT/target/debug/libforge_ffi.a" "$LIB_DIR/"

echo "[forge] Running swift test in $PACKAGE..."
swift test --package-path "$PACKAGE"
