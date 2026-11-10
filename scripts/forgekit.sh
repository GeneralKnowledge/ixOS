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
# The ForgeKit repository. Overridable only so tests can release into a
# throwaway repository.
REPOSITORY="${FORGEKIT_REPOSITORY:-https://github.com/GeneralKnowledge/ForgeKit.git}"
# github.com/Owner/Repo in any spelling (https, ssh, .git, case).
normalise_remote() {
  echo "$1" | tr '[:upper:]' '[:lower:]' \
    | sed -E 's#^(https?://|ssh://)?(git@)?##; s#^github\.com:#github.com/#; s#\.git/?$##; s#/$##'
}
swift_contract() {
  sed -n 's/^public let forgeKitContractVersion: UInt32 = \([0-9]*\).*/\1/p' "$KIT/Sources/ForgeKit/ForgeHost.swift"
}

generate_into() {
  local out="$1"
  # The cdylib is built only here: forge-app's Cargo.toml declares a plain
  # library, so apps never link a dylib of it (an iOS build on Linux cannot).
  cargo rustc -p forge-app --lib --crate-type cdylib >&2
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
    # The release names the ixOS commit it came from, so it must be that
    # commit, and one on ixOS main (a branch commit can vanish in a squash).
    if [[ -n "$(git status --porcelain -- forgekit crates/forge-app)" ]]; then
      echo "error: forgekit/ or crates/forge-app has uncommitted changes; commit them first" >&2
      exit 1
    fi
    if [[ -z "${FORGEKIT_ALLOW_UNMERGED:-}" ]]; then
      git fetch -q origin main
      if ! git merge-base --is-ancestor HEAD origin/main; then
        echo "error: HEAD is not on ixOS main; release from a merged commit" >&2
        exit 1
      fi
    fi
    version="$(forge_app_version)"

    # DEST must be a clean clone of the ForgeKit repository: nothing else is
    # ever overwritten.
    if [[ "$(git -C "$DEST" rev-parse --show-toplevel 2>/dev/null)" != "$(cd "$DEST" 2>/dev/null && pwd -P)" ]]; then
      echo "error: $DEST is not the top of a git clone" >&2
      exit 1
    fi
    remote="$(git -C "$DEST" remote get-url origin 2>/dev/null || true)"
    if [[ "$(normalise_remote "$remote")" != "$(normalise_remote "$REPOSITORY")" ]]; then
      echo "error: $DEST's origin is '${remote:-none}', not $REPOSITORY; refusing to touch it" >&2
      exit 1
    fi
    if [[ -n "$(git -C "$DEST" status --porcelain)" ]]; then
      echo "error: $DEST has uncommitted changes; refusing to touch it" >&2
      exit 1
    fi
    if git -C "$DEST" rev-parse -q --verify "refs/tags/$version" >/dev/null \
      || [[ -n "$(git -C "$DEST" ls-remote --tags origin "refs/tags/$version" 2>/dev/null)" ]]; then
      echo "error: ForgeKit $version is already released; bump forge-app's version first" >&2
      exit 1
    fi

    # The release is exactly the committed forgekit/: remove the tracked
    # files of the previous release, then unpack this one.
    git -C "$DEST" rm -rq --ignore-unmatch -- . >/dev/null
    git archive --format=tar HEAD:forgekit | tar -x -C "$DEST"
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
