#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

echo
echo "--- Android toolchain ---"

if command -v java >/dev/null; then
  java -version 2>&1 | head -1
else
  echo "MISSING: java (JDK 17+ recommended)"
fi

if command -v adb >/dev/null; then
  adb version | head -1
else
  echo "MISSING: adb (Android SDK platform-tools)"
fi

NDK_HOME="${ANDROID_NDK_HOME:-${NDK_HOME:-}}"
if [[ -n "$NDK_HOME" ]]; then
  echo "NDK_HOME=$NDK_HOME"
else
  echo "MISSING: ANDROID_NDK_HOME or NDK_HOME"
fi

if [[ -n "${ANDROID_HOME:-}" ]]; then
  echo "ANDROID_HOME=$ANDROID_HOME"
elif [[ -n "${ANDROID_SDK_ROOT:-}" ]]; then
  echo "ANDROID_SDK_ROOT=$ANDROID_SDK_ROOT"
else
  echo "MISSING: ANDROID_HOME or ANDROID_SDK_ROOT"
fi

if command -v cargo-ndk >/dev/null; then
  cargo ndk --version 2>/dev/null || echo "cargo-ndk present"
else
  echo "MISSING: cargo-ndk (cargo install cargo-ndk)"
fi

section_targets() {
  echo "Installed Android Rust targets:"
  if command -v rustup >/dev/null; then
    rustup target list --installed | grep -E 'android' || echo "(none)"
  fi
}
section_targets

SO="$ROOT/examples/counter-android/app/src/main/jniLibs/arm64-v8a/libforge_ffi.so"
if [[ -f "$SO" ]] && command -v readelf >/dev/null; then
  echo "16 KB page alignment probe on $SO:"
  if readelf -l "$SO" 2>/dev/null | grep -q '0x4000'; then
    echo "OK: LOAD segment with 0x4000 alignment found"
  else
    echo "WARN: no 0x4000 LOAD alignment detected; use NDK r28+ for Play 16 KB page size"
  fi
elif [[ -f "$SO" ]]; then
  echo "Built libforge_ffi.so present (install binutils readelf for alignment check)"
else
  echo "Native library not built yet; run scripts/build-rust-android.sh after NDK setup"
fi

echo "Android validation section complete."
