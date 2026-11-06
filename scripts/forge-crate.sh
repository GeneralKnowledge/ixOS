# Sourced by the build scripts: which Rust crate builds this app's native library.
#
# Apps created by `forge init` own their Rust crate (FORGE_APP_DIR/rust, or
# FORGE_RUST_DIR): it is built with its own Cargo.toml and target directory, and
# its native libraries are staged inside the app (FORGE_APP_DIR/build). Without
# FORGE_APP_DIR the scripts build the workspace's forge-ffi, which the bundled
# examples share.
#
# Sets: CRATE_DIR, CARGO_SELECT (array), CARGO_TARGET, NATIVE_OUT, SCAN_DIRS (array).
# Expects ROOT (the Forge workspace).

if [[ -n "${FORGE_APP_DIR:-}" ]]; then
  APP_DIR="$(cd "$FORGE_APP_DIR" && pwd)"
  CRATE_DIR="${FORGE_RUST_DIR:-$APP_DIR/rust}"
  if [[ ! -f "$CRATE_DIR/Cargo.toml" ]]; then
    echo "[forge] error: no Rust crate at $CRATE_DIR (expected Cargo.toml)." >&2
    echo "[forge]        forge.toml [rust] path names it; forge init creates it as rust/." >&2
    exit 1
  fi
  CRATE_DIR="$(cd "$CRATE_DIR" && pwd)"
  CARGO_SELECT=(--manifest-path "$CRATE_DIR/Cargo.toml")
  CARGO_TARGET="${CARGO_TARGET_DIR:-$CRATE_DIR/target}"
  NATIVE_OUT="$APP_DIR/build"
  # The app's crate plus the Forge crates it depends on by path.
  SCAN_DIRS=("$CRATE_DIR" "$ROOT/crates/forge-runtime" "$ROOT/crates/forge-net" "$ROOT/crates/forge-storage" "$ROOT/crates/forge-core")
else
  CRATE_DIR="$ROOT/crates/forge-ffi"
  CARGO_SELECT=(-p forge-ffi)
  CARGO_TARGET="${CARGO_TARGET_DIR:-$ROOT/target}"
  NATIVE_OUT="$ROOT/build"
  SCAN_DIRS=("$ROOT/crates/forge-ffi" "$ROOT/crates/forge-runtime" "$ROOT/crates/forge-net" "$ROOT/crates/forge-storage" "$ROOT/crates/forge-core" "$ROOT/examples/chat-core")
fi
