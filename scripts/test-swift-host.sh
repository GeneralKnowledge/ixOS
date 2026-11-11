#!/usr/bin/env bash
# Runs a Swift host package's XCTest suite on macOS against a host build of its
# Rust library (the app's own Rust crate, named by forge.toml [rust] path).
# Usage: scripts/test-swift-host.sh <package-dir>
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PACKAGE="${1:?usage: test-swift-host.sh <package-dir>}"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "error: Swift host tests need macOS (SwiftUI and Combine)" >&2
  exit 1
fi

cd "$ROOT"
PACKAGE_DIR="$(cd "$PACKAGE" && pwd)"
export FORGE_APP_DIR="${FORGE_APP_DIR:-$PACKAGE_DIR}"
# shellcheck source=forge-crate.sh
source "$ROOT/scripts/forge-crate.sh"
# Regenerate the Swift bindings from the current Rust (this also builds the
# host library), so the tests never use stale ones.
"$ROOT/scripts/generate-bindings.sh" "$PACKAGE_DIR/Generated" swift

# Only the static library goes in the search path, so the test binary does not
# need the .dylib at run time.
LIB_DIR="$NATIVE_OUT/macos-host/lib"
mkdir -p "$LIB_DIR"
cp "$CARGO_TARGET/debug/lib${LIB_NAME}.a" "$LIB_DIR/libforge_ffi.a"

# The Chat example's tests drive the real chat server (they skip the tests
# that need it when CHAT_SERVER is unset).
if [[ "$(basename "$PACKAGE")" == "chat" ]]; then
  echo "[forge] Building the chat server for the Chat tests..."
  (cd "$ROOT" && cargo build -p chat-server)
  export CHAT_SERVER="$ROOT/target/debug/chat-server"
fi

echo "[forge] Running swift test in $PACKAGE..."
swift test --package-path "$PACKAGE"
