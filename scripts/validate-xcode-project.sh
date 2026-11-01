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

# Resolve the build phase's REPO_ROOT and GENERATED the way Xcode would:
# SRCROOT is the folder that contains the .xcodeproj.
SRCROOT="$(cd "$(dirname "$PROJECT")/.." && pwd)"
SCRIPT="$(grep -o 'shellScript = ".*";' "$PROJECT" | grep 'package-ios-library' | head -1 |
  sed -e 's/^shellScript = "//' -e 's/";$//' -e 's/\\n/\n/g' -e 's/\\"/"/g')"
REPO_ROOT=""
GENERATED=""
eval "$(printf '%s\n' "$SCRIPT" | grep -E '^(REPO_ROOT|GENERATED)=')"

[[ -n "$REPO_ROOT" && -x "$REPO_ROOT/scripts/package-ios-library.sh" ]] || {
  echo "error: Rust FFI build phase REPO_ROOT ('$REPO_ROOT') is not the repository root" >&2
  exit 1
}

[[ -n "$GENERATED" && -f "$(dirname "$GENERATED")/Package.swift" ]] || {
  echo "error: Rust FFI build phase GENERATED ('$GENERATED') is not next to the app's Package.swift" >&2
  exit 1
}

echo "[forge] Xcode project structure OK: $PROJECT"
