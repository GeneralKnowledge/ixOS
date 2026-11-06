#!/usr/bin/env bash
# Runs a Swift host package's XCTest suite on macOS against a host build of its
# Rust library (the app's own rust/ crate, or the workspace's forge-ffi for the examples).
# Usage: scripts/test-swift-host.sh [package-dir]   (default: examples/counter)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PACKAGE="${1:-examples/counter}"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "error: Swift host tests need macOS (SwiftUI and Combine)" >&2
  exit 1
fi

cd "$ROOT"
PACKAGE_DIR="$(cd "$PACKAGE" && pwd)"
# Apps from `forge init` own their Rust crate (rust/); the examples share forge-ffi.
if [[ -z "${FORGE_APP_DIR:-}" && -f "$PACKAGE_DIR/rust/Cargo.toml" ]]; then
  export FORGE_APP_DIR="$PACKAGE_DIR"
fi
# shellcheck source=forge-crate.sh
source "$ROOT/scripts/forge-crate.sh"
if [[ -n "${FORGE_APP_DIR:-}" ]]; then
  # The app's own crate: regenerate its Swift bindings from the current Rust
  # (this also builds the host library), so the tests never use stale ones.
  "$ROOT/scripts/generate-bindings.sh" "$PACKAGE_DIR/Generated" swift
else
  echo "[forge] Building ${CRATE_DIR#"$ROOT/"} for the macOS host..."
  cargo build "${CARGO_SELECT[@]}"
fi

# Only the static library goes in the search path, so the test binary does not
# need the .dylib at run time.
LIB_DIR="$NATIVE_OUT/macos-host/lib"
mkdir -p "$LIB_DIR"
cp "$CARGO_TARGET/debug/libforge_ffi.a" "$LIB_DIR/"

# The Chat example's tests drive the real mock chat server (they skip the
# tests that need it when CHAT_MOCK_SERVER is unset).
if [[ "$(basename "$PACKAGE")" == "chat" ]]; then
  echo "[forge] Building the mock chat server for the Chat tests..."
  cargo build -p mock-chat-server
  export CHAT_MOCK_SERVER="$ROOT/target/debug/mock-chat-server"
fi

echo "[forge] Running swift test in $PACKAGE..."
swift test --package-path "$PACKAGE"
