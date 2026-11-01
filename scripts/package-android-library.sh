#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

GENERATED="${1:-${FORGE_GENERATED_DIR:-examples/counter-android/Generated}}"
LANGUAGE="${FORGE_BINDINGS_LANGUAGE:-kotlin}"

"$ROOT/scripts/generate-bindings.sh" "$GENERATED" "$LANGUAGE"

JNI_LIBS="${FORGE_ANDROID_JNI_LIBS:-$ROOT/examples/counter-android/app/src/main/jniLibs}"
ABIS="${FORGE_ANDROID_ABIS:-arm64-v8a x86_64}"

# Rebuild if any configured ABI is missing or stale, or an unconfigured one is still staged.
needs_rebuild=false
for abi in $ABIS; do
  if "$ROOT/scripts/native-lib-needs-rebuild.sh" "$JNI_LIBS/$abi/libforge_ffi.so" android; then
    needs_rebuild=true
  fi
done
for so in "$JNI_LIBS"/*/libforge_ffi.so; do
  [[ -f "$so" ]] || continue
  abi="$(basename "$(dirname "$so")")"
  if [[ " $ABIS " != *" $abi "* ]]; then
    needs_rebuild=true
  fi
done
if [[ "$needs_rebuild" == true ]]; then
  "$ROOT/scripts/build-rust-android.sh"
fi

echo "[forge] Android native libraries under ${JNI_LIBS}; Kotlin bindings in ${GENERATED}."
