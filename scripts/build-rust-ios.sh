#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
# shellcheck source=forge-crate.sh
source "$ROOT/scripts/forge-crate.sh"

# The SDKROOT the caller asked for, before this script resolves one from xcrun or
# xtool's Darwin SDK. The build metadata records this one, because that is what
# native-lib-needs-rebuild.sh sees on the next run; recording the resolved path
# made every Linux build look stale.
REQUESTED_SDKROOT="${SDKROOT:-}"

TARGET="${FORGE_IOS_TARGET:-aarch64-apple-ios}"
PROFILE="${FORGE_IOS_PROFILE:-release}"

if ! rustup target list --installed | grep -q "^${TARGET}$"; then
  echo "[forge] Adding Rust target ${TARGET}..."
  rustup target add "${TARGET}"
fi

SDKROOT_PATH=""
if [[ -z "${SDKROOT:-}" ]] && command -v xcrun >/dev/null 2>&1; then
  if IOS_SDK="$(xcrun --sdk iphoneos --show-sdk-path 2>/dev/null)"; then
    export SDKROOT="$IOS_SDK"
    export CC="$(xcrun --sdk iphoneos --find clang 2>/dev/null || true)"
    export CXX="$(xcrun --sdk iphoneos --find clang++ 2>/dev/null || true)"
    export AR="$(xcrun --sdk iphoneos --find ar 2>/dev/null || true)"
    export CARGO_TARGET_AARCH64_APPLE_IOS_LINKER="${CC:-}"
    echo "[forge] Using Xcode SDKROOT=$SDKROOT"
  fi
fi

if [[ -n "${SDKROOT:-}" ]]; then
  :
elif command -v swift >/dev/null 2>&1; then
  if swift sdk list 2>/dev/null | grep -q darwin; then
    CONFIG="$(swift sdk configure darwin "${TARGET}" --show-configuration 2>/dev/null || true)"
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

# Cargo calls the debug profile "dev" but writes it to target/<triple>/debug.
case "$PROFILE" in
  dev | debug) CARGO_PROFILE="dev" PROFILE_DIR="debug" ;;
  *) CARGO_PROFILE="$PROFILE" PROFILE_DIR="$PROFILE" ;;
esac

echo "[forge] Building ${CRATE_DIR#"$ROOT/"} for ${TARGET} (${CARGO_PROFILE})..."
# Only the static library is linked into the app. Asking for it alone skips the crate's
# cdylib, which needs an iOS linker that a Linux host (xtool's Darwin SDK) does not provide.
cargo rustc "${CARGO_SELECT[@]}" --lib --crate-type staticlib --target "$TARGET" --profile "$CARGO_PROFILE"

OUT_DIR="$NATIVE_OUT/ios-device/lib"
mkdir -p "$OUT_DIR"
cp "$CARGO_TARGET/${TARGET}/${PROFILE_DIR}/libforge_ffi.a" "$OUT_DIR/"
SDKROOT="$REQUESTED_SDKROOT" "$ROOT/scripts/write-native-build-meta.sh" "$OUT_DIR/libforge_ffi.a" ios
echo "[forge] Staged $OUT_DIR/libforge_ffi.a"
