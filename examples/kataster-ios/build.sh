#!/usr/bin/env bash
# Builds Kataster for iPhone on Linux and assembles an unsigned Kataster.app.
# This is the trial's route 1 (docs/bevy-game-trial.md): a Rust executable,
# linked with the clang and ld64.lld that xtool's Darwin SDK provides.
# Usage: build.sh [KATASTER_DIR]   (default: ./Kataster, from fetch.sh)
# Then sign and install with:  xtool install Kataster.app   (needs the phone)
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
DIR="${1:-$HERE/Kataster}"
TARGET=aarch64-apple-ios

CONFIG="$(swift sdk configure darwin arm64-apple-ios --show-configuration)"
SDKROOT="$(awk -F': ' '/sdkRootPath/ {print $2; exit}' <<<"$CONFIG")"
[[ -d "$SDKROOT" ]] || { echo "Darwin SDK not found; run xtool setup" >&2; exit 1; }
# The SDK bundle's toolset/bin/ld64.lld is xtool's wrapper (it reroutes -r
# links); call the real linker next to it.
BUNDLE="${SDKROOT%%/Developer/*}"
LD64="$BUNDLE/toolset/bin/orig/ld64.lld"
[[ -x "$LD64" ]] || LD64="$BUNDLE/toolset/bin/ld64.lld"
CLANG="$(dirname "$(readlink -f "$(command -v swift)")")/clang"
LIBCLANG_DIR="${LIBCLANG_PATH:-$(dirname "$(ls /usr/lib/llvm-*/lib/libclang.so* 2>/dev/null | sort -V | tail -1)")}"

export SDKROOT COREAUDIO_SDK_PATH="$SDKROOT" IPHONEOS_DEPLOYMENT_TARGET=17.0
export CC_aarch64_apple_ios="$CLANG" CXX_aarch64_apple_ios="${CLANG}++"
export AR_aarch64_apple_ios="$(dirname "$CLANG")/llvm-ar"
export CARGO_TARGET_AARCH64_APPLE_IOS_LINKER="$CLANG"
export CARGO_TARGET_AARCH64_APPLE_IOS_RUSTFLAGS="-C link-arg=-fuse-ld=lld -C link-arg=--ld-path=$LD64"
# coreaudio-sys (Bevy 0.17 audio) runs bindgen against the SDK headers.
export LIBCLANG_PATH="$LIBCLANG_DIR"
export BINDGEN_EXTRA_CLANG_ARGS_aarch64_apple_ios="--target=arm64-apple-ios17.0 -isysroot $SDKROOT"
# Kataster's .cargo/config.toml asks for /usr/bin/clang as the Linux host linker.
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="${HOST_LINKER:-cc}"
# Kataster has no rust-toolchain file; use the one ixOS pins.
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-$(awk -F'"' '/channel/ {print $2}' "$HERE/../../rust-toolchain.toml")}"

rustup target add "$TARGET" >/dev/null
(cd "$DIR" && cargo build --release --target "$TARGET" --bin kataster)

APP="$HERE/Kataster.app"
rm -rf "$APP" && mkdir -p "$APP"
cp "$DIR/target/$TARGET/release/kataster" "$APP/kataster"
# The Swift toolchain ships llvm-objcopy but not llvm-strip.
"$(dirname "$CLANG")/llvm-objcopy" --strip-all "$APP/kataster"
cp -R "$DIR/assets" "$APP/assets"
cp "$HERE/Info.plist" "$APP/Info.plist"
echo "Assembled $APP (unsigned). Install with: xtool install $APP"
