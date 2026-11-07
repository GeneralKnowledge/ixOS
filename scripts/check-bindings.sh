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
  for KOTLIN_FILE in uniffi/forge_ffi/forge_ffi.kt org/forgemobile/ffi/ForgeAndroid.kt; do
    if ! diff -u "$app/Generated/$KOTLIN_FILE" "$out/$KOTLIN_FILE"; then
      echo "error: $app/Generated/$KOTLIN_FILE is stale; run scripts/generate-bindings.sh $app/Generated kotlin" >&2
      exit 1
    fi
  done
  checked+=("$app")
done

echo "[forge] Kotlin bindings match committed copies: ${checked[*]}"
