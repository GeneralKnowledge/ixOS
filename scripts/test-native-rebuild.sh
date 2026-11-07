#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# The first checks use the Counter example's own crate.
export FORGE_APP_DIR="$ROOT/examples/counter"

FAKE="$TMP/libforge_ffi.a"
touch "$FAKE"
"$ROOT/scripts/write-native-build-meta.sh" "$FAKE" ios

if "$ROOT/scripts/native-lib-needs-rebuild.sh" "$FAKE" ios; then
  echo "error: expected fresh artifact to skip rebuild" >&2
  exit 1
fi

LIBRS="$ROOT/examples/counter/rust/src/lib.rs"
# Remember the source mtime so the probe below only bumps it temporarily; the
# file contents are never modified.
touch -r "$LIBRS" "$TMP/librs-mtime"
trap 'touch -r "$TMP/librs-mtime" "$LIBRS" 2>/dev/null || true; rm -rf "$TMP"' EXIT

sleep 1
touch "$LIBRS"

if ! "$ROOT/scripts/native-lib-needs-rebuild.sh" "$FAKE" ios; then
  echo "error: expected stale detection after source touch" >&2
  exit 1
fi

touch -r "$TMP/librs-mtime" "$LIBRS"

touch "$TMP/missing.so"
if ! "$ROOT/scripts/native-lib-needs-rebuild.sh" "$TMP/missing.so" android; then
  echo "error: missing artifact should require rebuild" >&2
  exit 1
fi

# Regression: build-rust-ios.sh resolves SDKROOT itself (xtool's Darwin SDK on
# Linux, xcrun on macOS), but the next run's check sees only what the caller
# set. A library it just built must count as fresh, or every `forge run ios`
# rebuilds the Rust library. Stub tools stand in for cargo, rustup and swift.
APP="$TMP/app"
STUBS="$TMP/stubs"
mkdir -p "$APP/rust" "$STUBS"
printf '[package]\nname = "stub"\n' >"$APP/rust/Cargo.toml"
cat >"$STUBS/rustup" <<'STUB'
#!/usr/bin/env bash
echo aarch64-apple-ios
STUB
cat >"$STUBS/swift" <<'STUB'
#!/usr/bin/env bash
case "$*" in
  "sdk list") echo darwin ;;
  *show-configuration*) echo "sdkRootPath: /stub/iPhoneOS.sdk" ;;
esac
STUB
cat >"$STUBS/cargo" <<'STUB'
#!/usr/bin/env bash
mkdir -p "$STUB_TARGET/aarch64-apple-ios/release"
echo stub >"$STUB_TARGET/aarch64-apple-ios/release/libforge_ffi.a"
STUB
chmod +x "$STUBS"/*
(
  unset SDKROOT
  export PATH="$STUBS:$PATH" FORGE_APP_DIR="$APP" STUB_TARGET="$APP/rust/target"
  "$ROOT/scripts/build-rust-ios.sh" >/dev/null
  if "$ROOT/scripts/native-lib-needs-rebuild.sh" "$APP/build/ios-device/lib/libforge_ffi.a" ios; then
    echo "error: a library build-rust-ios.sh just built is reported stale" >&2
    exit 1
  fi
)

echo "[forge] native-lib-needs-rebuild.sh checks passed."
