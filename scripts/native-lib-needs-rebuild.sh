#!/usr/bin/env bash
# Exit 0 if the native artifact should be rebuilt, 1 if it is up to date.
set -euo pipefail

usage() {
  echo "usage: native-lib-needs-rebuild.sh <artifact-path> ios|android" >&2
  exit 2
}

[[ $# -ge 2 ]] || usage

ARTIFACT="$1"
MODE="$2"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

if [[ ! -f "$ARTIFACT" ]]; then
  exit 0
fi

artifact_mtime() {
  stat -c %Y "$ARTIFACT" 2>/dev/null || stat -f %m "$ARTIFACT"
}

ART_MTIME="$(artifact_mtime)"

newer_than_artifact() {
  local path="$1"
  if [[ -f "$path" ]]; then
    local m
    m="$(stat -c %Y "$path" 2>/dev/null || stat -f %m "$path")"
    [[ "$m" -gt "$ART_MTIME" ]] && return 0
  fi
  return 1
}

scan_tree() {
  local dir="$1"
  [[ -d "$dir" ]] || return 0
  while IFS= read -r -d '' f; do
    if newer_than_artifact "$f"; then
      return 0
    fi
  done < <(find "$dir" -type f \( -name '*.rs' -o -name 'Cargo.toml' \) -print0)
  return 1
}

for crate in forge-core forge-storage forge-ffi; do
  if scan_tree "$ROOT/crates/$crate"; then
    exit 0
  fi
done

if newer_than_artifact "$ROOT/Cargo.lock"; then
  exit 0
fi

META="${ARTIFACT}.forge-build-meta"
case "$MODE" in
  ios)
    TARGET="${FORGE_IOS_TARGET:-aarch64-apple-ios}"
    PROFILE="${FORGE_IOS_PROFILE:-release}"
    WANT="${TARGET}|${PROFILE}|${SDKROOT:-}"
    ;;
  android)
    ABIS="${FORGE_ANDROID_ABIS:-arm64-v8a x86_64}"
    PROFILE="${FORGE_ANDROID_PROFILE:-release}"
    WANT="${ABIS}|${PROFILE}|${ANDROID_NDK_HOME:-${NDK_HOME:-}}"
    ;;
  *)
    usage
    ;;
esac

if [[ ! -f "$META" ]] || [[ "$(cat "$META")" != "$WANT" ]]; then
  exit 0
fi

exit 1
