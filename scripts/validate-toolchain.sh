#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

echo "=== Forge Mobile toolchain validation ==="
echo "Date: $(date -u +"%Y-%m-%dT%H:%M:%SZ")"
echo "Host: $(uname -srmo)"
echo

section() { echo; echo "--- $1 ---"; }

section "Rust"
if command -v rustc >/dev/null; then
  rustc --version
  cargo --version
  echo "Installed targets:"
  rustup target list --installed
else
  echo "MISSING: rustc/cargo"
fi

section "Swift"
if command -v swift >/dev/null; then
  swift --version
  if swift sdk list 2>/dev/null; then
    :
  else
    echo "Swift SDK list unavailable"
  fi
else
  echo "MISSING: swift"
fi

section "xtool"
if command -v xtool >/dev/null; then
  xtool --version
  xtool --help | head -5
else
  echo "MISSING: xtool (see docs/linux-setup.md)"
fi

section "UniFFI bindgen"
if [[ -x "$ROOT/target/debug/uniffi-bindgen" ]] || cargo run -p uniffi-bindgen -- --help >/dev/null 2>&1; then
  echo "uniffi-bindgen available via cargo run -p uniffi-bindgen"
else
  echo "Build workspace first: cargo build -p uniffi-bindgen"
fi

section "usbmuxd"
if command -v usbmuxd >/dev/null; then
  usbmuxd --help 2>&1 | head -2 || true
else
  echo "MISSING: usbmuxd (device install from Linux)"
fi

section "iOS Rust cross-compile probe"
if rustup target list --installed | grep -q aarch64-apple-ios; then
  if [[ -n "${SDKROOT:-}" ]]; then
    echo "SDKROOT=$SDKROOT"
  elif command -v swift >/dev/null && swift sdk list 2>/dev/null | grep -q darwin; then
    echo "Darwin SDK present; run scripts/build-rust-ios.sh for full link test"
  else
    echo "BLOCKED: aarch64-apple-ios target installed but no Darwin SDK (xtool setup + Xcode.xip)"
  fi
else
  echo "Run: rustup target add aarch64-apple-ios"
fi

section "Device deployment"
if command -v xtool >/dev/null; then
  if command -v timeout >/dev/null; then
    timeout 5 xtool devices 2>/dev/null | head -5 || echo "No devices / auth not configured (xtool setup) or timed out"
  else
    echo "Skipping xtool devices (install 'timeout' for non-interactive check)"
  fi
fi

if [[ -x "$ROOT/scripts/validate-android-toolchain.sh" ]]; then
  "$ROOT/scripts/validate-android-toolchain.sh"
fi

echo
echo "Validation complete. See docs/toolchain-validation.md for recorded outcomes."
