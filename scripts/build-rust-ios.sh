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
# forge-dev: development builds (the default, and every `forge build/run/publish
# ios` without --release). release: --release builds, with the app crate's
# [profile.release] (link-time optimisation, one codegen unit: the smallest,
# fastest binary, and by far the slowest build). See docs/builds.md.
PROFILE="${FORGE_IOS_PROFILE:-forge-dev}"

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

# On Linux no linker can link an iOS dylib (ld64.lld refuses the platform,
# cc doesn't know -framework). The static library needs none, but Cargo
# links the cdylibs that dependencies such as iroh declare, so those get an
# empty file instead (see the script). Xcode's clang (xcrun) is a real iOS
# linker and stays.
if [[ "$(uname -s)" == "Linux" ]]; then
  export CARGO_TARGET_AARCH64_APPLE_IOS_LINKER="${FORGE_IOS_LINKER:-$(cd "$(dirname "$0")" && pwd)/ios-no-dylib-linker.sh}"
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

# The forge-dev profile: optimised like release (opt-level 3), but without
# link-time optimisation and with parallel codegen units and incremental
# compilation, so a change rebuilds in seconds to a minute instead of
# re-optimising the whole program on one core. An app crate may define its own
# [profile.forge-dev] in Cargo.toml; otherwise Forge defines it here.
PROFILE_CONFIG=()
if [[ "$CARGO_PROFILE" == forge-dev ]] && ! grep -q '^\[profile\.forge-dev\]' "$CRATE_DIR/Cargo.toml"; then
  PROFILE_CONFIG=(
    --config 'profile.forge-dev.inherits="release"'
    --config 'profile.forge-dev.lto=false'
    --config 'profile.forge-dev.codegen-units=16'
    --config 'profile.forge-dev.incremental=true'
  )
fi

echo "[forge] Building ${CRATE_DIR#"$ROOT/"} for ${TARGET} (${CARGO_PROFILE})..."
# Only the static library is linked into the app. Asking for it alone skips the crate's
# cdylib, which needs an iOS linker that a Linux host (xtool's Darwin SDK) does not provide.
# FORGE_RUST_FEATURES: the app crate's features, comma-separated (`forge run
# ios --devtools` sets `devtools`).
FEATURES=()
if [[ -n "${FORGE_RUST_FEATURES:-}" ]]; then
  FEATURES=(--features "$FORGE_RUST_FEATURES")
fi
# Line tables for the Rust frames of crash reports (`forge crash`, docs/forge-device.md).
# They stay in the static library's object files, which the app binary's debug map
# points at, so the app itself does not grow. FORGE_IOS_RUST_DEBUG overrides the level
# (`false` leaves them out); a CARGO_PROFILE_<NAME>_DEBUG set by the caller wins.
PROFILE_DEBUG_VAR="CARGO_PROFILE_$(echo "$CARGO_PROFILE" | tr 'a-z-' 'A-Z_')_DEBUG"
if [[ "$CARGO_PROFILE" != "dev" && -z "${!PROFILE_DEBUG_VAR:-}" ]]; then
  export "$PROFILE_DEBUG_VAR=${FORGE_IOS_RUST_DEBUG:-line-tables-only}"
fi
cargo rustc ${PROFILE_CONFIG[@]+"${PROFILE_CONFIG[@]}"} "${CARGO_SELECT[@]}" --lib --crate-type staticlib --target "$TARGET" --profile "$CARGO_PROFILE" ${FEATURES[@]+"${FEATURES[@]}"}

OUT_DIR="$NATIVE_OUT/ios-device/lib"
mkdir -p "$OUT_DIR"
# Staged under Forge's fixed name whatever the crate calls its library.
cp "$CARGO_TARGET/${TARGET}/${PROFILE_DIR}/lib${LIB_NAME}.a" "$OUT_DIR/libforge_ffi.a"
SDKROOT="$REQUESTED_SDKROOT" "$ROOT/scripts/write-native-build-meta.sh" "$OUT_DIR/libforge_ffi.a" ios
echo "[forge] Staged $OUT_DIR/libforge_ffi.a"
