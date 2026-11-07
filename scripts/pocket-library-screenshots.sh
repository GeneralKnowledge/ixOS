#!/usr/bin/env bash
# Screenshots of Pocket Library in the iOS Simulator, so its UI can be
# reviewed without a device: the empty library, then a seeded one (books in
# every status, covers, two weeks of history) on each tab, in light and dark,
# and the Library and Activity tabs at the largest accessibility text size.
#
# The simulator is a review aid, not part of the supported workflow (the app
# is built with xtool on Linux and run on an iPhone), and these are not
# device results. macOS CI runs this; see the "Pocket Library screenshots"
# job in .github/workflows/macos.yml. Adapted from PR #32's
# scripts/ios-screenshots.sh; fold into it once that merges.
#
# Usage: scripts/pocket-library-screenshots.sh <PocketLibrary.app> <out-dir>
set -euo pipefail

[[ $# -eq 2 ]] || { echo "usage: $0 <PocketLibrary.app> <out-dir>" >&2; exit 2; }
APP_PATH="$1"
OUT="$2"
BUNDLE_ID=org.forgemobile.pocketlibrary
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$OUT"

# The first available iPhone with "Pro" in its name, else any iPhone.
DEVICE="$(xcrun simctl list devices available -j | python3 -c '
import json, sys
devices = [d for runtime, ds in json.load(sys.stdin)["devices"].items()
           if "iOS" in runtime for d in ds if d["name"].startswith("iPhone")]
pro = [d for d in devices if "Pro" in d["name"] and "Max" not in d["name"]]
print((pro or devices)[0]["udid"])
')"
echo "[screenshots] Using simulator $DEVICE"
xcrun simctl boot "$DEVICE" 2>/dev/null || true
xcrun simctl bootstatus "$DEVICE" -b >/dev/null
xcrun simctl status_bar "$DEVICE" override --time 9:41 --batteryState charged --batteryLevel 100 \
  --cellularBars 4 --wifiBars 3 >/dev/null 2>&1 || true
xcrun simctl install "$DEVICE" "$APP_PATH"

library_dir() {
  local data
  data="$(xcrun simctl get_app_container "$DEVICE" "$BUNDLE_ID" data)"
  echo "$data/Library/Application Support/PocketLibrary"
}

shoot() { # name tab
  for appearance in light dark; do
    xcrun simctl ui "$DEVICE" appearance "$appearance"
    xcrun simctl terminate "$DEVICE" "$BUNDLE_ID" >/dev/null 2>&1 || true
    # `-selectedTab` lands in UserDefaults' argument domain (ContentView's
    # @AppStorage), so the app opens on that tab.
    xcrun simctl launch "$DEVICE" "$BUNDLE_ID" -selectedTab "$2" >/dev/null
    sleep 4
    xcrun simctl io "$DEVICE" screenshot "$OUT/$1-$appearance.png" >/dev/null
    echo "[screenshots] $OUT/$1-$appearance.png"
  done
}

# First launch creates the container and an empty library.
xcrun simctl launch "$DEVICE" "$BUNDLE_ID" >/dev/null
sleep 4
xcrun simctl terminate "$DEVICE" "$BUNDLE_ID" >/dev/null 2>&1 || true
rm -rf "$(library_dir)"
shoot 1-library-empty library

# A realistic library, written by the app's own Rust code.
cargo run -q --manifest-path "$ROOT/examples/pocket-library/rust/Cargo.toml" \
  --example seed_screenshots -- "$(library_dir)"
shoot 2-library library
shoot 3-activity activity
shoot 4-settings settings

xcrun simctl ui "$DEVICE" content_size accessibility-extra-extra-extra-large
shoot 5-library-largest-text library
shoot 6-activity-largest-text activity
xcrun simctl ui "$DEVICE" content_size large
