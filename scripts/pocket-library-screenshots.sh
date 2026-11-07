#!/usr/bin/env bash
# Screenshots of Pocket Library in the iOS Simulator, so its UI can be
# reviewed without a device: the empty library, then a seeded one (books in
# every status, covers, two weeks of history) on each tab, in light and dark,
# and the Library and Activity tabs at the largest accessibility text size.
#
# The simulator is a review aid, not part of the supported workflow (the app
# is built with xtool on Linux and run on an iPhone), and these are not
# device results. macOS CI runs this; see the "Pocket Library screenshots"
# job in .github/workflows/macos.yml. Shares its helpers with
# ios-screenshots.sh (scripts/ios-simulator.sh).
#
# Usage: scripts/pocket-library-screenshots.sh <PocketLibrary.app> <out-dir>
set -euo pipefail

[[ $# -eq 2 ]] || { echo "usage: $0 <PocketLibrary.app> <out-dir>" >&2; exit 2; }
APP_PATH="$1"
OUT="$2"
BUNDLE_ID=org.forgemobile.pocketlibrary
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$OUT"
# shellcheck source=ios-simulator.sh
source "$ROOT/scripts/ios-simulator.sh"

sim_boot_and_install "$APP_PATH"
library_dir() { echo "$(sim_app_support "$BUNDLE_ID")/PocketLibrary"; }
# `-selectedTab` lands in UserDefaults' argument domain (ContentView's
# @AppStorage), so the app opens on that tab.
shoot() { sim_shoot "$OUT" "$1" "$BUNDLE_ID" -selectedTab "$2"; } # name tab

# First launch creates the container and an empty library.
sim_first_launch "$BUNDLE_ID"
rm -rf "$(library_dir)"
shoot 1-library-empty library

# A realistic library, written by the app's own Rust code.
cargo run -q --manifest-path "$ROOT/examples/pocket-library/rust/Cargo.toml" \
  --example seed_screenshots -- "$(library_dir)"
shoot 2-library library
shoot 3-activity activity
shoot 4-settings settings

sim_text_size accessibility-extra-extra-extra-large
shoot 5-library-largest-text library
shoot 6-activity-largest-text activity
sim_text_size large
