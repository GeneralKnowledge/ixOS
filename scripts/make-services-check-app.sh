#!/usr/bin/env bash
# Creates the native services device check: a fresh `forge init` iOS app whose
# screen exercises the Keychain, notifications, permissions, deep links,
# sharing and file import through the real Rust -> Swift path. Install it on
# an iPhone with `forge run ios` (see docs/native-services-device-check.md).
#
# Usage: scripts/make-services-check-app.sh [dir]   (default: ~/forge-services-check)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP="${1:-$HOME/forge-services-check}"
if [[ -e "$APP" ]]; then
  echo "error: $APP already exists; remove it or pass another directory" >&2
  exit 1
fi

cargo build -q --manifest-path "$ROOT/Cargo.toml" -p forge-cli
export FORGE_WORKSPACE="$ROOT"
"$ROOT/target/debug/forge" init ServicesCheck --platform ios \
  --bundle-id "${FORGE_CHECK_BUNDLE_ID:-org.example.servicescheck}" --path "$APP"

cat "$ROOT/crates/forge-services/device-check/services_check.rs" >>"$APP/rust/src/lib.rs"
cp "$ROOT/crates/forge-services/device-check/ContentView.swift" "$APP/Sources/AppHost/ContentView.swift"

# Register the forgecheck:// scheme (forge.toml [ios] url_schemes).
if grep -q 'url_schemes' "$APP/forge.toml"; then
  if grep -q '^\[ios\]$' "$APP/forge.toml"; then
    awk '{ print } /^\[ios\]$/ { print "url_schemes = [\"forgecheck\"]" }' "$APP/forge.toml" >"$APP/forge.toml.new"
    mv "$APP/forge.toml.new" "$APP/forge.toml"
  else
    printf '\n[ios]\nurl_schemes = ["forgecheck"]\n' >>"$APP/forge.toml"
  fi
fi

# Rewrite Info.plist and xtool.yml from forge.toml (forge run does this too).
"$ROOT/target/debug/forge" config sync --project "$APP"

cargo test -q --manifest-path "$APP/rust/Cargo.toml"
"$ROOT/target/debug/forge" bindings generate --project "$APP"
echo
echo "[forge] Services check app ready in $APP"
echo "[forge] Next: forge run ios --project $APP   (iPhone plugged in and unlocked)"
