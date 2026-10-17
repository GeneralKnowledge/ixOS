#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROJECT="${1:-$ROOT/examples/counter/Xcode/Counter.xcodeproj/project.pbxproj}"

if [[ ! -f "$PROJECT" ]]; then
  echo "error: missing Xcode project at $PROJECT" >&2
  exit 1
fi

grep -q 'XCLocalSwiftPackageReference' "$PROJECT" || {
  echo "error: project missing local Swift package reference" >&2
  exit 1
}

grep -q 'PRODUCT_BUNDLE_IDENTIFIER' "$PROJECT" || {
  echo "error: project missing bundle identifier" >&2
  exit 1
}

grep -q 'Build Rust FFI' "$PROJECT" || {
  echo "error: project missing Rust FFI build phase" >&2
  exit 1
}

echo "[forge] Xcode project structure OK: $PROJECT"
