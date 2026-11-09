#!/usr/bin/env bash
# Phase 0 spike: generate the app's Swift bindings with the app's own
# uniffi-bindgen (rust/bindgen), no Forge checkout involved.
#
#   scripts/bindings.sh               app bindings into Generated/
#   scripts/bindings.sh --framework DIR   also write the forge_app bindings
#                                     into a ForgeKit checkout at DIR (what a
#                                     Forge release would do once per version)
#
# UniFFI generates every component in the library: forge_ffi (the app) and
# forge_app (the shared runtime contract). The app keeps only its own; the
# forge_app Swift lives in ForgeKit as the ForgeApp module, and the app's
# bindings import it.
set -euo pipefail
APP="$(cd "$(dirname "$0")/.." && pwd)"
FRAMEWORK=""
if [[ "${1:-}" == "--framework" ]]; then FRAMEWORK="$2"; fi

cargo build --manifest-path "$APP/rust/Cargo.toml" -p spike-app-core
case "$(uname -s)" in Darwin) EXT=dylib ;; *) EXT=so ;; esac
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
cargo run --quiet --manifest-path "$APP/rust/Cargo.toml" -p uniffi-bindgen -- generate \
  --library "$APP/rust/target/debug/libforge_ffi.$EXT" --language swift --out-dir "$STAGE" \
  2>&1 | grep -v "swift-format" || true

mkdir -p "$APP/Generated/ForgeFFI" "$APP/Generated/forge_ffiFFI"
# UniFFI writes no `import` for a Swift module holding external types (it
# assumes one module); add it. The one line Forge's tooling must own.
sed 's/^import Foundation$/import Foundation\n@preconcurrency import ForgeApp/' \
  "$STAGE/forge_ffi.swift" >"$APP/Generated/ForgeFFI/forge_ffi.swift"
cp "$STAGE/forge_ffiFFI.h" "$APP/Generated/forge_ffiFFI/forge_ffiFFI.h"
cp "$STAGE/forge_ffiFFI.modulemap" "$APP/Generated/forge_ffiFFI/module.modulemap"

if [[ -n "$FRAMEWORK" ]]; then
  mkdir -p "$FRAMEWORK/Sources/ForgeApp" "$FRAMEWORK/Sources/forge_appFFI"
  cp "$STAGE/forge_app.swift" "$FRAMEWORK/Sources/ForgeApp/forge_app.swift"
  cp "$STAGE/forge_appFFI.h" "$FRAMEWORK/Sources/forge_appFFI/forge_appFFI.h"
  cp "$STAGE/forge_appFFI.modulemap" "$FRAMEWORK/Sources/forge_appFFI/module.modulemap"
fi
echo "bindings generated"
