#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PROFILE="${FORGE_ANDROID_PROFILE:-release}"
JNI_LIBS="${FORGE_ANDROID_JNI_LIBS:-$ROOT/examples/counter-android/app/src/main/jniLibs}"

# Space-separated Gradle ABI folder names (arm64-v8a, x86_64, armeabi-v7a).
ABIS="${FORGE_ANDROID_ABIS:-arm64-v8a x86_64}"

if ! command -v cargo-ndk >/dev/null 2>&1; then
  echo "[forge] error: cargo-ndk not found. Install with: cargo install cargo-ndk" >&2
  exit 1
fi

NDK_HOME="${ANDROID_NDK_HOME:-${NDK_HOME:-}}"
if [[ -z "$NDK_HOME" ]]; then
  echo "[forge] error: set ANDROID_NDK_HOME or NDK_HOME to your Android NDK installation." >&2
  exit 1
fi
export ANDROID_NDK_HOME="$NDK_HOME"

declare -a NDK_TARGETS=()
for abi in $ABIS; do
  case "$abi" in
    arm64-v8a)
      rustup target add aarch64-linux-android 2>/dev/null || true
      NDK_TARGETS+=("arm64-v8a")
      ;;
    x86_64)
      rustup target add x86_64-linux-android 2>/dev/null || true
      NDK_TARGETS+=("x86_64")
      ;;
    armeabi-v7a)
      rustup target add armv7-linux-androideabi 2>/dev/null || true
      NDK_TARGETS+=("armeabi-v7a")
      ;;
    *)
      echo "[forge] error: unknown ABI '$abi' (use arm64-v8a, x86_64, armeabi-v7a)" >&2
      exit 1
      ;;
  esac
done

mkdir -p "$JNI_LIBS"

echo "[forge] Building forge-ffi for Android (${PROFILE}) ABIs: ${NDK_TARGETS[*]}..."

declare -a NDK_ARGS=()
for abi in "${NDK_TARGETS[@]}"; do
  NDK_ARGS+=(-t "$abi")
done

declare -a CARGO_PROFILE_ARGS=()
if [[ "$PROFILE" == "release" ]]; then
  CARGO_PROFILE_ARGS=(--release)
else
  CARGO_PROFILE_ARGS=(--profile "$PROFILE")
fi

export ANDROID_NDK_ROOT="${ANDROID_NDK_ROOT:-$NDK_HOME}"

cargo ndk \
  "${NDK_ARGS[@]}" \
  -o "$JNI_LIBS" \
  build -p forge-ffi "${CARGO_PROFILE_ARGS[@]}"

for abi in "${NDK_TARGETS[@]}"; do
  so="$JNI_LIBS/$abi/libforge_ffi.so"
  if [[ ! -f "$so" ]]; then
    echo "[forge] error: expected native library at $so" >&2
    exit 1
  fi
  echo "[forge] Staged $so"
done
