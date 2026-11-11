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
    # The native services' Swift files: the same set, with the same contents.
    if ! diff -u \
        <(cd "$app/Generated/ForgeFFI" && ls ForgeServices*.swift 2>/dev/null || true) \
        <(cd "$out/ForgeFFI" && ls ForgeServices*.swift 2>/dev/null || true); then
      echo "error: $app/Generated/ForgeFFI has a different set of Forge Swift files; run: forge bindings generate --project $app" >&2
      exit 1
    fi
    for file in "$out"/ForgeFFI/ForgeServices*.swift; do
      [[ -f "$file" ]] || continue
      if ! diff -u "$app/Generated/ForgeFFI/$(basename "$file")" "$file"; then
        echo "error: $app/Generated/ForgeFFI/$(basename "$file") is stale; run: forge bindings generate --project $app" >&2
        exit 1
      fi
    done
    # Other app crates linked into this one (Device Lab's showcase): the
    # re-export list and each crate's Swift and C modules.
    linked_list="ForgeFFI/ForgeFFI+Linked.swift"
    if [[ -f "$out/$linked_list" || -f "$app/Generated/$linked_list" ]]; then
      if ! diff -u "$app/Generated/$linked_list" "$out/$linked_list"; then
        echo "error: $app/Generated/$linked_list is stale; run: forge bindings generate --project $app" >&2
        exit 1
      fi
      while read -r _ _ ns; do
        if ! diff -ru "$app/Generated/$ns" "$out/$ns" || ! diff -ru "$app/Generated/${ns}FFI" "$out/${ns}FFI"; then
          echo "error: $app/Generated/$ns is stale; run: forge bindings generate --project $app" >&2
          exit 1
        fi
      done < <(grep '^@_exported import ' "$out/$linked_list")
    fi
  fi
done

echo "[forge] App crates OK: ${APPS[*]}"
