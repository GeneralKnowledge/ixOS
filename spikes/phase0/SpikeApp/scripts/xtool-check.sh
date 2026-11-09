#!/usr/bin/env bash
# Phase 1 gate: build this app for iOS with xtool on Linux, the real
# developer workflow, with ForgeKit as a package dependency (exact 0.3.0)
# and the app's Rust library from git. Builds only; installs nothing.
#
#   scripts/xtool-check.sh <path to an ixOS checkout at this branch>
#
# The checkout is used only to stand in for a published ForgeKit (a local
# git repository tagged 0.3.0), exactly as CI does. The app is copied
# elsewhere first, so nothing else of the checkout is involved.
set -euo pipefail
SRC="$(cd "$1" && pwd)"
WORK="${XTOOL_CHECK_DIR:-$HOME/forge-phase0-xtool}"
rm -rf "$WORK" && mkdir -p "$WORK"

cp -R "$SRC/spikes/phase0/ForgeKit" "$WORK/ForgeKit"
(cd "$WORK/ForgeKit" && git init -q -b main && git add . \
  && git -c user.name=check -c user.email=check@example.invalid commit -qm "ForgeKit 0.3.0" && git tag 0.3.0)
cp -R "$SRC/spikes/phase0/SpikeApp" "$WORK/SpikeApp"
cd "$WORK/SpikeApp"
rm -rf rust/target build .build xtool
sed -i "s#https://github.com/GeneralKnowledge/ForgeKit.git#file://$WORK/ForgeKit#" Package.swift

echo "== versions"; xtool --version 2>/dev/null || true; swift --version 2>&1 | head -1; rustc --version

echo "== Rust for aarch64-apple-ios with the Darwin SDK"
CONFIG="$(swift sdk configure darwin arm64-apple-ios --show-configuration 2>/dev/null)"
export SDKROOT="$(echo "$CONFIG" | awk -F': ' '/sdkRootPath/ {print $2; exit}')"
TOOLCHAIN="$(echo "$CONFIG" | awk -F': ' '/toolchainPath/ {print $2; exit}')"
export CC="$TOOLCHAIN/usr/bin/clang" CXX="$TOOLCHAIN/usr/bin/clang++" AR="$TOOLCHAIN/usr/bin/ar"
export CARGO_TARGET_AARCH64_APPLE_IOS_LINKER="$CC"
rustup target add aarch64-apple-ios >/dev/null
(cd rust && cargo rustc --locked -p spike-app-core --lib --crate-type staticlib --target aarch64-apple-ios --release)
mkdir -p build/ios-device/lib
cp rust/target/aarch64-apple-ios/release/libforge_ffi.a build/ios-device/lib/

echo "== xtool build (no install)"
START=$(date +%s)
xtool dev build
echo "== xtool build OK in $(( $(date +%s) - START ))s"
find xtool -maxdepth 3 -name "*.app" -print
