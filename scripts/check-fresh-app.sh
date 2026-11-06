#!/usr/bin/env bash
# Proves that a brand-new app works outside the Forge checkout, through the
# public CLI only: `forge init --path` for iOS and Android, the app's own Rust
# crate tested and linted on its own, a change to its Rust API, and
# `forge bindings generate --project` picking that change up.
#
# Runs on Linux with no Xcode, Android SDK or device. The platform halves run
# in CI: XCTest on macOS (macos.yml) and Gradle unit tests plus a debug APK
# (android.yml), against apps created the same way.
#
# Usage: scripts/check-fresh-app.sh [work-dir]   (default: a new temp dir)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="${1:-$(mktemp -d)}"
mkdir -p "$WORK"
WORK="$(cd "$WORK" && pwd)"
case "$WORK/" in
  "$ROOT"/*)
    echo "error: work dir must be outside the checkout ($ROOT)" >&2
    exit 1
    ;;
esac

step() { echo; echo "[fresh-app] $*"; }
fail() { echo "[fresh-app] FAIL: $*" >&2; exit 1; }

step "Building the forge CLI"
cargo build -q --manifest-path "$ROOT/Cargo.toml" -p forge-cli
FORGE="$ROOT/target/debug/forge"
export FORGE_WORKSPACE="$ROOT"
before="$(git -C "$ROOT" status --porcelain)"

cd "$WORK"
step "forge init (iOS and Android) in $WORK"
"$FORGE" init FreshIos --bundle-id org.example.freshios --path "$WORK/FreshIos"
"$FORGE" init FreshDroid --platform android --bundle-id org.example.freshdroid --path "$WORK/FreshDroid"

for app in FreshIos FreshDroid; do
  dir="$WORK/$app"
  step "$app: files and placeholders"
  for f in forge.toml rust/Cargo.toml rust/Cargo.lock rust/src/lib.rs Generated; do
    [[ -e "$dir/$f" ]] || fail "$app is missing $f"
  done
  if grep -rIl 'FORGE_\(PACKAGE_NAME\|APP_NAME\|BUNDLE_ID\|APP_SUPPORT_DIR\|JAVA_PACKAGE\|WORKSPACE\)' \
    --exclude-dir=target --exclude-dir=Generated "$dir"; then
    fail "$app still has template placeholders (files above)"
  fi
  grep -q '^path = "rust"' "$dir/forge.toml" || fail "$app forge.toml does not name its crate"

  step "$app: the app's own crate (cargo test, clippy, fmt)"
  manifest="$dir/rust/Cargo.toml"
  cargo test -q --manifest-path "$manifest"
  cargo clippy -q --manifest-path "$manifest" --all-targets -- -D warnings
  cargo fmt --manifest-path "$manifest" --check
done

step "Nothing was written into the checkout"
after="$(git -C "$ROOT" status --porcelain)"
[[ "$before" == "$after" ]] || fail "forge init changed the checkout:
$(diff <(echo "$before") <(echo "$after") || true)"

step "Changing the Rust API reaches the bindings (forge bindings generate --project)"
for app in FreshIos FreshDroid; do
  cat >>"$WORK/$app/rust/src/lib.rs" <<'RUST'

/// Added by check-fresh-app.sh: a new export must appear in the bindings.
#[uniffi::export]
pub fn fresh_word_count(text: String) -> u32 {
    text.split_whitespace().count() as u32
}
RUST
  "$FORGE" bindings generate --project "$WORK/$app"
done
grep -q 'func freshWordCount(text: String) ' "$WORK/FreshIos/Generated/ForgeFFI/forge_ffi.swift" \
  || fail "Swift bindings do not have freshWordCount"
grep -q 'fun `freshWordCount`(`text`: kotlin.String)' "$WORK/FreshDroid/Generated/uniffi/forge_ffi/forge_ffi.kt" \
  || fail "Kotlin bindings do not have freshWordCount"
[[ -f "$WORK/FreshDroid/Generated/org/forgemobile/ffi/ForgeAndroid.kt" ]] \
  || fail "Android app is missing ForgeAndroid.kt"

step "OK: fresh iOS and Android apps in $WORK own and build their Rust logic"
