#!/usr/bin/env bash
# Checks that every Android example's committed Kotlin bindings match what its
# Rust crate generates now. (Swift bindings of the iOS examples are checked
# with their crates by check-app-crates.sh.)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
cd "$ROOT"

checked=()
for app in examples/*/; do
  app="${app%/}"
  [[ -f "$app/forge.toml" && -f "$app/build.gradle.kts" ]] || continue
  out="$TMP/$(basename "$app")"
  FORGE_APP_DIR="$app" "$ROOT/scripts/generate-bindings.sh" "$out" kotlin >/dev/null
  # Every generated file: an app linking another crate (Pocket Library's
  # Android app) gets one Kotlin file per UniFFI namespace.
  if ! diff -ru "$app/Generated" "$out"; then
    echo "error: $app/Generated is stale; run scripts/generate-bindings.sh $app/Generated kotlin" >&2
    exit 1
  fi
  checked+=("$app")
done

echo "[forge] Kotlin bindings match committed copies: ${checked[*]}"
