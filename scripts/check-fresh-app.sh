#!/usr/bin/env bash
# Proves that a brand-new app works outside the Forge checkout, through the
# public CLI only: `forge init --path` for iOS and Android, `forge init <Name>`
# into the current directory with `forge test` run from inside the app, the
# app's own Rust crate tested and linted on its own, a change to its Rust API,
# and `forge bindings generate --project` picking that change up.
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
step "forge init (iOS, minimal iOS and Android) in $WORK"
"$FORGE" init FreshIos --bundle-id org.example.freshios --path "$WORK/FreshIos"
"$FORGE" init FreshMini --minimal --bundle-id org.example.freshmini --path "$WORK/FreshMini"
"$FORGE" init FreshDroid --platform android --bundle-id org.example.freshdroid --path "$WORK/FreshDroid"

for app in FreshIos FreshMini FreshDroid; do
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
  if [[ "$app" != FreshDroid ]]; then
    # The iOS starters are on the application runtime: a forge-app core,
    # hosted by ForgeKit (docs/app-model-review.md).
    grep -q '^forge-app = ' "$dir/rust/Cargo.toml" || fail "$app's crate does not use forge-app"
    grep -q 'forge_app::export_runtime!' "$dir/rust/src/lib.rs" || fail "$app's core does not export the runtime"
    grep -qE '\.package\((path: "[^"]*/forgekit"|url: "https://github.com/GeneralKnowledge/ForgeKit.git")' "$dir/Package.swift" \
      || fail "$app's Package.swift does not depend on ForgeKit"
    grep -q 'ForgeHost(' "$dir/Sources/AppHost/AppHost.swift" || fail "$app is not hosted by ForgeKit"
  fi
  if [[ "$app" == FreshMini ]]; then
    # The minimal starter has no native services or lifecycle: nothing may still use them.
    if grep -rlE 'AppServices|receivesDeepLinks|forgeLifecycle|NotesModel' --include='*.swift' "$dir/Sources" "$dir/Xcode" "$dir/Tests"; then
      fail "FreshMini still refers to the full starter (files above)"
    fi
  fi

  step "$app: the app's own crate (cargo test, clippy, fmt)"
  manifest="$dir/rust/Cargo.toml"
  cargo test -q --manifest-path "$manifest"
  cargo clippy -q --manifest-path "$manifest" --all-targets -- -D warnings
  # Development builds (forge run ios --devtools) add the live connection.
  cargo test -q --manifest-path "$manifest" --features devtools
  cargo clippy -q --manifest-path "$manifest" --all-targets --features devtools -- -D warnings
  cargo fmt --manifest-path "$manifest" --check
done

step "forge init without --path creates ./<Name>; forge test runs from inside it"
"$FORGE" init PlainIos --bundle-id org.example.plainios >"$WORK/plain-init.log"
[[ -f "$WORK/PlainIos/forge.toml" ]] || fail "forge init PlainIos did not create ./PlainIos"
grep -q 'cd PlainIos' "$WORK/plain-init.log" || fail "forge init did not print the next steps"
grep -q '^# PlainIos$' "$WORK/PlainIos/README.md" || fail "PlainIos/README.md is missing or not named"
(cd "$WORK/PlainIos" && "$FORGE" test)
if ! command -v xtool >/dev/null 2>&1; then
  # Without xtool, `forge run ios` must stop before the Rust build and say how to fix it.
  if (cd "$WORK/PlainIos" && "$FORGE" run ios) >"$WORK/plain-run.log" 2>&1; then
    fail "forge run ios succeeded without xtool"
  fi
  grep -q 'cannot start yet' "$WORK/plain-run.log" || fail "forge run ios did not explain the missing toolchain: $(cat "$WORK/plain-run.log")"
  [[ ! -e "$WORK/PlainIos/build" ]] || fail "forge run ios started building before the preflight"
fi

step "Nothing was written into the checkout"
after="$(git -C "$ROOT" status --porcelain)"
[[ "$before" == "$after" ]] || fail "forge init changed the checkout:
$(diff <(echo "$before") <(echo "$after") || true)"

step "Changing the Rust API reaches the bindings (forge bindings generate --project)"
for app in FreshIos FreshMini FreshDroid; do
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

if [[ -n "${FORGE_SWIFT_DOCKER:-}" ]] || command -v swift >/dev/null 2>&1; then
  step "forge check swift: the view model and bindings compile, and their tests pass, without the iOS SDK"
  "$FORGE" check swift --project "$WORK/FreshIos"
  "$FORGE" check swift --project "$WORK/FreshMini"
  step "forge check swift catches a Swift call that no longer matches the Rust API (exit 11)"
  model="$WORK/FreshIos/Sources/AppHost/NotesModel.swift"
  cp "$model" "$model.orig"
  sed -i.bak '0,/\.addNote(text:/s//.addNote(body:/' "$model" && rm -f "$model.bak"
  set +e
  "$FORGE" check swift --project "$WORK/FreshIos" --json >"$WORK/check-swift.json" 2>"$WORK/check-swift.log"
  code=$?
  set -e
  mv "$model.orig" "$model"
  [[ $code -eq 11 ]] || fail "forge check swift exited $code, not 11: $(cat "$WORK/check-swift.log")"
  grep -q '"file":"Sources/AppHost/NotesModel.swift"' "$WORK/check-swift.json" \
    || fail "forge check swift --json did not point at NotesModel.swift: $(cat "$WORK/check-swift.json")"
else
  step "Skipping forge check swift: no swift on PATH and FORGE_SWIFT_DOCKER unset"
fi

step "iOS app configuration comes from forge.toml (forge config sync / check)"
ios="$WORK/FreshIos"
for f in Info.plist App.entitlements xtool.yml; do
  grep -q 'Generated by Forge from forge.toml' "$ios/$f" || fail "FreshIos/$f is not generated from forge.toml"
done
grep -q '^bundleID: org.example.freshios$' "$ios/xtool.yml" || fail "xtool.yml has the wrong bundle ID"
"$FORGE" config check --project "$ios"
cat >>"$ios/forge.toml" <<'TOML'

[ios.entitlements.extra]
"com.apple.developer.default-data-protection" = "NSFileProtectionComplete"
TOML
sed -i.bak -e 's/^# camera = .*/camera = "Scan barcodes to add notes."/' \
  -e 's/^version = .*/version = "1.2.0"/' \
  -e 's/^minimum_ios = .*/minimum_ios = "18.0"/' "$ios/forge.toml"
rm -f "$ios/forge.toml.bak"
if "$FORGE" config check --project "$ios" 2>/dev/null; then
  fail "forge config check passed after forge.toml changed"
fi
"$FORGE" config sync --project "$ios"
"$FORGE" config check --project "$ios"
grep -q '<string>Scan barcodes to add notes.</string>' "$ios/Info.plist" || fail "camera permission text missing from Info.plist"
grep -q '<string>1.2.0</string>' "$ios/Info.plist" || fail "version missing from Info.plist"
grep -q 'NSFileProtectionComplete' "$ios/App.entitlements" || fail "entitlement missing from App.entitlements"
grep -q '^entitlementsPath: App.entitlements$' "$ios/xtool.yml" || fail "xtool.yml does not apply App.entitlements"
grep -q '\.iOS("18.0")' "$ios/Package.swift" || fail "Package.swift platform does not follow minimum_ios"

step "OK: fresh iOS and Android apps in $WORK own and build their Rust logic"
