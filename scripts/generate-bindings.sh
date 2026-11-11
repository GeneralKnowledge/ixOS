#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
# shellcheck source=forge-crate.sh
source "$ROOT/scripts/forge-crate.sh"

OUT="${1:-$APP_DIR/Generated}"
LANGUAGE="${2:-swift}"

case "$LANGUAGE" in
  swift | kotlin) ;;
  *)
    echo "error: unsupported language '$LANGUAGE' (use swift or kotlin)" >&2
    exit 1
    ;;
esac

mkdir -p "$OUT"

echo "[forge] Building ${CRATE_DIR#"$ROOT/"} (host) for binding generation..."
if grep -q '^crate-type *=.*"cdylib"' "$CRATE_DIR/Cargo.toml"; then
  cargo build "${CARGO_SELECT[@]}"
else
  # An app crate other apps link (Weather, ...) has no cdylib: as a
  # dependency, Cargo would link it for iOS, which needs Apple's linker.
  # UniFFI reads the bindings from a host shared library, so build one here.
  cargo rustc "${CARGO_SELECT[@]}" --lib --crate-type cdylib
fi

case "$(uname -s)" in
  Darwin) LIB_EXT="dylib" ;;
  *) LIB_EXT="so" ;;
esac
LIB="$CARGO_TARGET/debug/lib${LIB_NAME}.${LIB_EXT}"
# The bindings' Swift module is SWIFT_BINDINGS (forge.toml `[swift]
# bindings_module`, else ForgeFFI) over the C module <library>FFI. The files
# keep Forge's fixed names (Generated/ForgeFFI/forge_ffi.swift,
# Generated/forge_ffiFFI/forge_ffiFFI.h), so the tools find them in every
# app. Kotlin stays uniffi.forge_ffi loading libforge_ffi.
CONFIG="$(mktemp)"
if [[ "$LIB_NAME" != forge_ffi || "$SWIFT_BINDINGS" != ForgeFFI ]]; then
  {
    echo "[crates.${LIB_NAME}.bindings.swift]"
    echo "module_name = \"${SWIFT_BINDINGS}\""
    echo "ffi_module_name = \"${LIB_NAME}FFI\""
    echo 'ffi_module_filename = "forge_ffiFFI"'
    echo ""
    echo "[crates.${LIB_NAME}.bindings.kotlin]"
    echo 'package_name = "uniffi.forge_ffi"'
    echo 'cdylib_name = "forge_ffi"'
  } >"$CONFIG"
fi
if [[ ! -f "$LIB" ]]; then
  echo "error: expected host library at $LIB" >&2
  exit 1
fi

echo "[forge] Generating ${LANGUAGE} bindings into $OUT..."
if [[ "$LANGUAGE" == "kotlin" ]]; then
  cargo run -p uniffi-bindgen -- generate \
    --library "$LIB" \
    --config "$CONFIG" \
    --language kotlin \
    --out-dir "$OUT"
  rm -f "$CONFIG"
  # Hand-written Android glue that travels with the bindings (ForgeAndroid.init:
  # the app Context for the HTTPS certificate verifier). Source of truth:
  # crates/forge-runtime/kotlin/.
  mkdir -p "$OUT/org/forgemobile/ffi"
  cp "$ROOT/crates/forge-runtime/kotlin/org/forgemobile/ffi/ForgeAndroid.kt" "$OUT/org/forgemobile/ffi/ForgeAndroid.kt"
else
  # SwiftPM needs the Swift source and the C module in separate folders:
  #   Generated/ForgeFFI/forge_ffi.swift                  (Swift target "ForgeFFI")
  #   Generated/forge_ffiFFI/{forge_ffiFFI.h,module.modulemap}  (system library "forge_ffiFFI")
  STAGE="$(mktemp -d)"
  trap 'rm -rf "$STAGE" "$CONFIG"' EXIT
  cargo run -p uniffi-bindgen -- generate \
    --library "$LIB" \
    --config "$CONFIG" \
    --language swift \
    --out-dir "$STAGE"
  # With the module_name override UniFFI names the file after the module.
  if [[ -f "$STAGE/${SWIFT_BINDINGS}.swift" ]]; then
    mv "$STAGE/${SWIFT_BINDINGS}.swift" "$STAGE/forge_ffi.swift"
  fi
  # One app's Swift module: DIR/ForgeFFI/<file>.swift over the C module
  # DIR/<ffi>FFI/, plus the Swift helpers, native services adapters and
  # logging wrapper its bindings call for.
  forge_swift_module() {
    local dir="$1" ns="$2" ffi="$3"
    local swift="$dir/ForgeFFI/$ns.swift"
    rm -rf "$dir/ForgeFFI"
    mkdir -p "$dir/ForgeFFI" "$dir/${ffi}FFI"
    if [[ -f "$STAGE/forge_app.swift" ]]; then
      # The crate uses the application runtime (forge-app). Its contract
      # types' Swift ships in ForgeKit's ForgeApp module, not here; UniFFI
      # writes no import for a module of external types, so add it.
      sed 's/^import Foundation$/import Foundation\n@preconcurrency import ForgeApp/' \
        "$STAGE/$ns.swift" >"$swift"
    else
      cp "$STAGE/$ns.swift" "$swift"
    fi
    mv "$STAGE/${ffi}FFI.h" "$dir/${ffi}FFI/${ffi}FFI.h"
    mv "$STAGE/${ffi}FFI.modulemap" "$dir/${ffi}FFI/module.modulemap"
    # Swift logging: crates calling `forge_runtime::log_export!()` export
    # forgeLog, and get the ForgeLog wrapper. Source of truth:
    # crates/forge-runtime/swift/ForgeLog.swift.
    if grep -q 'public func forgeLog(' "$swift"; then
      cp "$ROOT/crates/forge-runtime/swift/ForgeLog.swift" "$dir/ForgeFFI/ForgeLog.swift"
      echo "[forge] Added ForgeLog (Swift logging into the Rust log pipeline)."
    fi
    # Swift helpers with no Rust side and no privacy-sensitive framework
    # (ImageIO thumbnails, the audio session, web views), for every app. Source of truth:
    # crates/forge-services/swift/helpers/.
    cp "$ROOT/crates/forge-services/swift/helpers/"*.swift "$dir/ForgeFFI/"
    # Native services: when the crate uses `forge_services::native_services!()`,
    # Forge's Swift adapters (Keychain, notifications, ...) compile in the same
    # module as the bindings, so they can implement its protocols. Source of
    # truth: crates/forge-services/swift/. Crates without the macro get none.
    #
    # Optional services (`native_services!(connectivity, motion, ...)`) each
    # have one adapter in swift/optional/ForgeServices+<Name>.swift, copied only
    # when the bindings contain its `installNative<Name>` function, so an app
    # links only the frameworks (Core Location, Core Motion, ...) it selected.
    # ForgeServices+Selected.swift lists their installers for
    # ForgeServices.install().
    if grep -q 'public func installNativeServices(' "$swift"; then
      cp "$ROOT/crates/forge-services/swift/"ForgeServices*.swift "$dir/ForgeFFI/"
      local selected=() adapter name
      for adapter in "$ROOT/crates/forge-services/swift/optional/"ForgeServices+*.swift; do
        [[ -f "$adapter" ]] || continue
        name="${adapter##*/ForgeServices+}"
        name="${name%.swift}"
        if grep -q "public func installNative${name}(" "$swift"; then
          cp "$adapter" "$dir/ForgeFFI/"
          selected+=("$name")
        fi
      done
      {
        echo "// Generated by scripts/generate-bindings.sh: the optional native services this"
        echo "// app's Rust crate selects in forge_services::native_services!(...). Do not edit."
        echo ""
        echo "public let forgeSelectedServices: [(name: String, install: @Sendable () throws -> Void)] = ["
        for name in ${selected[@]+"${selected[@]}"}; do
          echo "    (\"${name}\", ForgeServices.install${name}),"
        done
        echo "]"
      } >"$dir/ForgeFFI/ForgeServices+Selected.swift"
      if [[ ${#selected[@]} -gt 0 ]]; then
        echo "[forge] Added the native services Swift adapters, with: ${selected[*]}."
      else
        echo "[forge] Added the native services Swift adapters."
      fi
    fi
  }

  forge_swift_module "$OUT" forge_ffi forge_ffi
  # Older layout kept everything at the top level, which SwiftPM rejects.
  rm -f "$OUT/forge_ffi.swift" "$OUT/forge_ffiFFI.h" "$OUT/forge_ffiFFI.modulemap"

  # Other app crates linked into this library (Device Lab links Weather,
  # Checklist, ...) are Swift packages of their own, built from their own
  # Generated/ folders, so their bindings are left out here.
  rm -rf "$OUT/Linked"
fi

echo "[forge] Bindings generated."
