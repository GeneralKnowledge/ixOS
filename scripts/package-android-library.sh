#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

GENERATED="${1:-${FORGE_GENERATED_DIR:-examples/counter-android/Generated}}"
LANGUAGE="${FORGE_BINDINGS_LANGUAGE:-kotlin}"

"$ROOT/scripts/generate-bindings.sh" "$GENERATED" "$LANGUAGE"

JNI_LIBS="${FORGE_ANDROID_JNI_LIBS:-$ROOT/examples/counter-android/app/src/main/jniLibs}"
PROBE="$JNI_LIBS/arm64-v8a/libforge_ffi.so"
if [[ ! -f "$PROBE" ]]; then
  "$ROOT/scripts/build-rust-android.sh"
fi

echo "[forge] Android native libraries under ${JNI_LIBS}; Kotlin bindings in ${GENERATED}."
