#!/usr/bin/env bash
# Usage: verify-android-packaging.sh [project-dir]
# Checks that every ABI listed in the project's forge.toml (or FORGE_ANDROID_ABIS)
# has a staged libforge_ffi.so, that no other ABI does, and that bindings exist.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROJECT="${1:-$ROOT/examples/counter-android}"
JNI="$PROJECT/app/src/main/jniLibs"

abis_from_forge_toml() {
  local toml="$PROJECT/forge.toml"
  [[ -f "$toml" ]] || return 0
  # abis = ["arm64-v8a", "x86_64"]  ->  arm64-v8a x86_64
  grep -E '^[[:space:]]*abis[[:space:]]*=' "$toml" | head -1 |
    sed -e 's/^[^[]*\[//' -e 's/\].*$//' -e 's/[",]/ /g' | xargs
}

ABIS="${FORGE_ANDROID_ABIS:-$(abis_from_forge_toml)}"
ABIS="${ABIS:-arm64-v8a x86_64}"

for abi in $ABIS; do
  so="$JNI/$abi/libforge_ffi.so"
  if [[ ! -f "$so" ]]; then
    echo "error: missing $so (run package-android-library.sh)" >&2
    exit 1
  fi
done

for so in "$JNI"/*/libforge_ffi.so; do
  [[ -f "$so" ]] || continue
  abi="$(basename "$(dirname "$so")")"
  if [[ " $ABIS " != *" $abi "* ]]; then
    echo "error: $so is staged but $abi is not a configured ABI ($ABIS)" >&2
    exit 1
  fi
done

KOTLIN="$PROJECT/Generated/uniffi/forge_ffi/forge_ffi.kt"
if [[ ! -f "$KOTLIN" ]]; then
  echo "error: missing generated Kotlin bindings at $KOTLIN" >&2
  exit 1
fi

echo "[forge] Android jniLibs ($ABIS) and Kotlin bindings present for $PROJECT"
