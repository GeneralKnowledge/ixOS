# Sourced by the build scripts: which Rust crate builds this app's native library.
#
# Every app owns its Rust crate (FORGE_APP_DIR/rust, or FORGE_RUST_DIR, or the
# `[rust] path` in its forge.toml): it is built with its own Cargo.toml and
# target directory, and its native libraries are staged inside the app
# (FORGE_APP_DIR/build). Without FORGE_APP_DIR the app is found from the
# script's first argument: the app directory itself, or its Generated/ folder.
#
# Sets: APP_DIR, CRATE_DIR, CARGO_SELECT (array), LIB_NAME, SWIFT_BINDINGS, CARGO_TARGET, NATIVE_OUT, SCAN_DIRS (array).
# Expects ROOT (the Forge workspace).

# The `path` under [rust] in a forge.toml, if any.
forge_toml_rust_path() {
  sed -n '/^\[rust\]/,/^\[/ s/^path *= *"\([^"]*\)".*/\1/p' "$1" | head -n 1
}

if [[ -z "${FORGE_APP_DIR:-}" ]]; then
  for candidate in "${1:-}" "$(dirname "${1:-.}")"; do
    if [[ -n "$candidate" && -f "$candidate/forge.toml" ]]; then
      export FORGE_APP_DIR="$candidate"
      break
    fi
  done
fi
if [[ -z "${FORGE_APP_DIR:-}" ]]; then
  echo "[forge] error: no app given. Pass the app directory (or its Generated/ folder), or set FORGE_APP_DIR." >&2
  exit 1
fi
if [[ -z "${FORGE_RUST_DIR:-}" && -f "$FORGE_APP_DIR/forge.toml" ]]; then
  rust_path="$(forge_toml_rust_path "$FORGE_APP_DIR/forge.toml")"
  if [[ -n "$rust_path" ]]; then
    FORGE_RUST_DIR="$FORGE_APP_DIR/$rust_path"
  fi
fi

APP_DIR="$(cd "$FORGE_APP_DIR" && pwd)"
CRATE_DIR="${FORGE_RUST_DIR:-$APP_DIR/rust}"
if [[ ! -f "$CRATE_DIR/Cargo.toml" ]]; then
  echo "[forge] error: no Rust crate at $CRATE_DIR (expected Cargo.toml)." >&2
  echo "[forge]        forge.toml [rust] path names it; forge init creates it as rust/." >&2
  exit 1
fi
CRATE_DIR="$(cd "$CRATE_DIR" && pwd)"
CARGO_SELECT=(--manifest-path "$CRATE_DIR/Cargo.toml")
# The crate's library name (`[lib] name`, else the package name). Each app may
# name its own library (weather_core, ...), so that another app can link it;
# whatever the name, the library is staged as libforge_ffi.a.
LIB_NAME="$(sed -n '/^\[lib\]/,/^\[/ s/^name *= *"\([^"]*\)".*/\1/p' "$CRATE_DIR/Cargo.toml" | head -n 1)"
if [[ -z "$LIB_NAME" ]]; then
  LIB_NAME="$(sed -n '/^\[package\]/,/^\[/ s/^name *= *"\([^"]*\)".*/\1/p' "$CRATE_DIR/Cargo.toml" | head -n 1 | tr - _)"
fi
# The Swift module of the app's bindings: forge.toml `[swift] bindings_module`,
# else ForgeFFI. An app another app hosts names its own (WeatherFFI, ...).
SWIFT_BINDINGS=""
if [[ -f "$APP_DIR/forge.toml" ]]; then
  SWIFT_BINDINGS="$(sed -n '/^\[swift\]/,/^\[/ s/^bindings_module *= *"\([^"]*\)".*/\1/p' "$APP_DIR/forge.toml" | head -n 1)"
fi
SWIFT_BINDINGS="${SWIFT_BINDINGS:-ForgeFFI}"
CARGO_TARGET="${CARGO_TARGET_DIR:-$CRATE_DIR/target}"
NATIVE_OUT="$APP_DIR/build"
# The app's crate plus the Forge crates it depends on by path.
SCAN_DIRS=("$CRATE_DIR" "$ROOT/crates/forge-runtime" "$ROOT/crates/forge-services" "$ROOT/crates/forge-net" "$ROOT/crates/forge-storage")
# Plus every other crate the app's Cargo.toml names by path (Chat's
# chat-core, say), so editing one of those also rebuilds the library.
while IFS= read -r dep; do
  dep_dir="$(cd "$CRATE_DIR" && cd "$dep" 2>/dev/null && pwd)" || continue
  [[ " ${SCAN_DIRS[*]} " == *" $dep_dir "* ]] || SCAN_DIRS+=("$dep_dir")
done < <(sed -nE 's/.*[{ ,]path *= *"([^"]+)".*/\1/p' "$CRATE_DIR/Cargo.toml")
