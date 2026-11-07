#!/usr/bin/env bash
# Screenshots of the Weather example in the iOS Simulator, so its UI can be
# reviewed without a device: "My Location" and four saved cities with
# different skies (cloudy, showers, sunny, snow, a clear night), in light and
# dark mode, the places list, and a page at the largest accessibility text
# size. The data is seeded (rust/examples/seed_screenshots.rs) and the app is
# launched with -WeatherOfflineDemo YES, so it never asks for location or
# uses the network.
#
# The simulator is a review aid, not part of the supported workflow (the app
# is built with xtool on Linux and run on an iPhone), and these are not
# device results. macOS CI runs this; see the "Weather screenshots" job in
# .github/workflows/screenshots.yml.
#
# Usage: scripts/weather-screenshots.sh <Weather.app> <out-dir>
set -euo pipefail

[[ $# -eq 2 ]] || { echo "usage: $0 <Weather.app> <out-dir>" >&2; exit 2; }
APP_PATH="$1"
OUT="$2"
BUNDLE_ID=org.forgemobile.weather
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$OUT"
# shellcheck source=ios-simulator.sh
source "$ROOT/scripts/ios-simulator.sh"

sim_boot_and_install "$APP_PATH"
data_dir() { echo "$(sim_app_support "$BUNDLE_ID")/Weather"; }
# Launch arguments land in UserDefaults' argument domain.
shoot() { sim_shoot "$OUT" "$1" "$BUNDLE_ID" -WeatherOfflineDemo YES -WeatherPage "$2" "${@:3}"; } # name page [args]

# First launch creates the container; the seed then fills it.
xcrun simctl launch "$SIM_DEVICE" "$BUNDLE_ID" -WeatherOfflineDemo YES >/dev/null
sleep 4
xcrun simctl terminate "$SIM_DEVICE" "$BUNDLE_ID" >/dev/null 2>&1 || true
cargo run -q --manifest-path "$ROOT/examples/weather/rust/Cargo.toml" \
  --example seed_screenshots -- "$(data_dir)"

shoot 1-my-location-cloudy 0
shoot 2-showers-coming 1
shoot 3-sunny 2
shoot 4-snow 3
shoot 5-clear-night 4
shoot 6-places-list 0 -WeatherShowPlaces YES

sim_text_size accessibility-extra-extra-extra-large
shoot 7-largest-text 1
sim_text_size large
