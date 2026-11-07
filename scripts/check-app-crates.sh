#!/usr/bin/env bash
# Checks every app-owned Rust crate in the checkout (examples/*/rust, made by
# `forge init`): rustfmt, clippy and tests, then that its committed Swift
# bindings match what the crate generates now. These crates have their own
# Cargo workspace, so the workspace-wide `cargo fmt/clippy/test` and
# check-bindings.sh do not reach them.
# Usage: scripts/check-app-crates.sh [app-dir...]   (default: every examples/*/rust)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [[ $# -gt 0 ]]; then
  APPS=("$@")
else
  APPS=()
  for manifest in examples/*/rust/Cargo.toml; do
    [[ -f "$manifest" ]] && APPS+=("${manifest%/rust/Cargo.toml}")
  done
fi

if [[ ${#APPS[@]} -eq 0 ]]; then
  echo "[forge] No app-owned crates under examples/."
  exit 0
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

for app in "${APPS[@]}"; do
  manifest="$app/rust/Cargo.toml"
  echo "[forge] $app: fmt, clippy, test"
  cargo fmt --manifest-path "$manifest" -- --check
  cargo clippy --manifest-path "$manifest" --all-targets -- -D warnings
  cargo test --manifest-path "$manifest"

  if [[ -f "$app/Package.swift" ]]; then
    out="$TMP/$(basename "$app")"
    FORGE_APP_DIR="$app" "$ROOT/scripts/generate-bindings.sh" "$out" swift >/dev/null
    for file in ForgeFFI/forge_ffi.swift forge_ffiFFI/forge_ffiFFI.h forge_ffiFFI/module.modulemap; do
      if ! diff -u "$app/Generated/$file" "$out/$file"; then
        echo "error: $app/Generated/$file is stale; run: forge bindings generate --project $app" >&2
        exit 1
      fi
    done
  fi
done

echo "[forge] App crates OK: ${APPS[*]}"
