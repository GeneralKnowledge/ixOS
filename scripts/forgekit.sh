#!/usr/bin/env bash
# ForgeKit, the Swift half of the application runtime, is made here.
#
#   scripts/forgekit.sh generate        regenerate forgekit/Sources/ForgeApp and
#                                       forgekit/Sources/forge_appFFI from crates/forge-app
#   scripts/forgekit.sh check           the committed bindings match, and the Rust
#                                       and Swift contract versions agree
#   scripts/forgekit.sh release DIR     check, then write the release into DIR (a
#                                       clone of GeneralKnowledge/ForgeKit), commit it
#                                       and tag it with the forge-app version
#
# forgekit/ in this repository is the only place ForgeKit changes. The
# GeneralKnowledge/ForgeKit repository exists because SwiftPM needs a
# package at a repository root; it holds releases and nothing else.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
KIT="$ROOT/forgekit"
CRATE="$ROOT/crates/forge-app"

forge_app_version() {
  sed -n 's/^version = "\([^"]*\)".*/\1/p' "$CRATE/Cargo.toml" | head -n 1
}
rust_contract() {
  sed -n 's/^pub const CONTRACT_VERSION: u32 = \([0-9]*\);.*/\1/p' "$CRATE/src/contract.rs"
}
swift_contract() {
  sed -n 's/^public let forgeKitContractVersion: UInt32 = \([0-9]*\).*/\1/p' "$KIT/Sources/ForgeKit/ForgeHost.swift"
}

generate_into() {
  local out="$1"
  cargo build -p forge-app >&2
  case "$(uname -s)" in Darwin) ext=dylib ;; *) ext=so ;; esac
  local stage
  stage="$(mktemp -d)"
  cargo run --quiet -p uniffi-bindgen -- generate \
    --library "$ROOT/target/debug/libforge_app.$ext" --language swift --out-dir "$stage" >&2
  mkdir -p "$out/ForgeApp" "$out/forge_appFFI"
  mv "$stage/forge_app.swift" "$out/ForgeApp/forge_app.swift"
  mv "$stage/forge_appFFI.h" "$out/forge_appFFI/forge_appFFI.h"
  mv "$stage/forge_appFFI.modulemap" "$out/forge_appFFI/module.modulemap"
  rm -rf "$stage"
}

check() {
  local tmp rc=0
  tmp="$(mktemp -d)"
  generate_into "$tmp"
  for file in ForgeApp/forge_app.swift forge_appFFI/forge_appFFI.h forge_appFFI/module.modulemap; do
    if ! diff -u "$KIT/Sources/$file" "$tmp/$file"; then
      echo "error: forgekit/Sources/$file is stale; run scripts/forgekit.sh generate" >&2
      rc=1
    fi
  done
  rm -rf "$tmp"
  local rust swift
  rust="$(rust_contract)"
  swift="$(swift_contract)"
  if [[ -z "$rust" || "$rust" != "$swift" ]]; then
    echo "error: contract versions differ: forge-app CONTRACT_VERSION=$rust, ForgeKit forgeKitContractVersion=$swift" >&2
    rc=1
  fi
  [[ $rc -eq 0 ]] && echo "[forge] ForgeKit $(forge_app_version) matches forge-app (contract $rust)."
  return $rc
}

case "${1:-}" in
  generate)
    generate_into "$KIT/Sources"
    echo "[forge] ForgeKit bindings generated."
    ;;
  check)
    check
    ;;
  release)
    DEST="${2:?usage: scripts/forgekit.sh release DIR}"
    check
    # The release names the ixOS commit it came from, so it must be that commit.
    if [[ -n "$(git status --porcelain -- forgekit crates/forge-app)" ]]; then
      echo "error: forgekit/ or crates/forge-app has uncommitted changes; commit them first" >&2
      exit 1
    fi
    version="$(forge_app_version)"
    [[ -d "$DEST/.git" ]] || { echo "error: $DEST is not a git clone of the ForgeKit repository" >&2; exit 1; }
    if git -C "$DEST" rev-parse -q --verify "refs/tags/$version" >/dev/null; then
      echo "error: ForgeKit $version is already released; bump forge-app's version first" >&2
      exit 1
    fi
    # The release is exactly forgekit/: replace everything but .git.
    find "$DEST" -mindepth 1 -maxdepth 1 ! -name .git -exec rm -rf {} +
    cp -R "$KIT"/. "$DEST"/
    source_rev="$(git rev-parse HEAD)"
    cat >"$DEST/RELEASE" <<INFO
ForgeKit $version, contract $(rust_contract)
Made from GeneralKnowledge/ixOS $source_rev (forgekit/) by scripts/forgekit.sh.
Do not edit this repository: change forgekit/ in ixOS and release again.
INFO
    git -C "$DEST" add -A
    git -C "$DEST" commit -qm "ForgeKit $version (ixOS ${source_rev:0:12})"
    git -C "$DEST" tag -a "$version" -m "ForgeKit $version, contract $(rust_contract), from ixOS $source_rev"
    echo "[forge] Released ForgeKit $version into $DEST. Push with: git -C $DEST push origin HEAD $version"
    ;;
  *)
    sed -n '2,12p' "$0" >&2
    exit 1
    ;;
esac
