#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "usage: write-native-build-meta.sh <artifact-path> ios|android" >&2
  exit 2
}

[[ $# -ge 2 ]] || usage

ARTIFACT="$1"
MODE="$2"
META="${ARTIFACT}.forge-build-meta"

case "$MODE" in
  ios)
    TARGET="${FORGE_IOS_TARGET:-aarch64-apple-ios}"
    PROFILE="${FORGE_IOS_PROFILE:-release}"
    printf '%s' "${TARGET}|${PROFILE}|${SDKROOT:-}" >"$META"
    ;;
  android)
    ABIS="${FORGE_ANDROID_ABIS:-arm64-v8a x86_64}"
    PROFILE="${FORGE_ANDROID_PROFILE:-release}"
    printf '%s' "${ABIS}|${PROFILE}|${ANDROID_NDK_HOME:-${NDK_HOME:-}}" >"$META"
    ;;
  *)
    usage
    ;;
esac
