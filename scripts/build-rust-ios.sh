#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

TARGET="${FORGE_IOS_TARGET:-aarch64-apple-ios}"
PROFILE="${FORGE_IOS_PROFILE:-release}"

if ! rustup target list --installed | grep -q "^${TARGET}$"; then
  echo "[forge] Adding Rust target ${TARGET}..."
  rustup target add "${TARGET}"
fi

SDKROOT_PATH=""
if command -v swift >/dev/null 2>&1; then
  if swift sdk list 2>/dev/null | grep -q darwin; then
    CONFIG="$(swift sdk configure darwin "${TARGET/arm64-/arm64-}" --show-configuration 2>/dev/null || true)"
    if [[ -z "$CONFIG" ]]; then
      CONFIG="$(swift sdk configure darwin arm64-apple-ios --show-configuration 2>/dev/null || true)"
    fi
    SDKROOT_PATH="$(echo "$CONFIG" | awk -F': ' '/sdkRootPath/ {print $2; exit}')"
    TOOLCHAIN="$(echo "$CONFIG" | awk -F': ' '/toolchainPath/ {print $2; exit}')"
    if [[ -n "${SDKROOT_PATH:-}" ]]; then
      export SDKROOT="$SDKROOT_PATH"
      echo "[forge] Using SDKROOT=$SDKROOT"
    fi
    if [[ -n "${TOOLCHAIN:-}" && -x "${TOOLCHAIN}/usr/bin/clang" ]]; then
      export CC="${TOOLCHAIN}/usr/bin/clang"
      export CXX="${TOOLCHAIN}/usr/bin/clang++"
      export AR="${TOOLCHAIN}/usr/bin/ar"
      export CARGO_TARGET_AARCH64_APPLE_IOS_LINKER="${TOOLCHAIN}/usr/bin/clang"
    fi
  else
    echo "[forge] warning: Darwin Swift SDK not installed (run: xtool setup)" >&2
  fi
else
  echo "[forge] warning: swift not found; set SDKROOT manually for iOS Rust builds" >&2
fi

if [[ -z "${SDKROOT:-}" ]]; then
  echo "[forge] error: SDKROOT is not set. Install the Darwin SDK via xtool setup and Xcode.xip." >&2
  exit 1
fi

echo "[forge] Building forge-ffi for ${TARGET} (${PROFILE})..."
cargo build -p forge-ffi --target "$TARGET" --profile "$PROFILE"

OUT_DIR="$ROOT/build/ios-device/lib"
mkdir -p "$OUT_DIR"
cp "$ROOT/target/${TARGET}/${PROFILE}/libforge_ffi.a" "$OUT_DIR/"
echo "[forge] Staged $OUT_DIR/libforge_ffi.a"
