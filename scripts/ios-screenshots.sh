#!/usr/bin/env bash
# Takes screenshots of a built Forge starter app in the iOS Simulator, so its
# UI can be inspected without a device: empty, with notes, the recovery
# screen, and the largest accessibility text size, each in light and dark.
#
# The simulator is a review aid, not part of the supported workflow (apps are
# built with xtool on Linux and run on an iPhone). macOS CI runs this; see
# the "Starter app screenshots" job in .github/workflows/macos.yml.
#
# Usage: scripts/ios-screenshots.sh <App.app> <bundle-id> <app-name> <out-dir>
set -euo pipefail

[[ $# -eq 4 ]] || { echo "usage: $0 <App.app> <bundle-id> <app-name> <out-dir>" >&2; exit 2; }
APP_PATH="$1"
BUNDLE_ID="$2"
APP_NAME="$3"
OUT="$4"
mkdir -p "$OUT"
# shellcheck source=ios-simulator.sh
source "$(dirname "$0")/ios-simulator.sh"

sim_boot_and_install "$APP_PATH"
store() { echo "$(sim_app_support "$BUNDLE_ID")/$APP_NAME/notes.json"; }

now_ms=$(( $(date +%s) * 1000 ))
day=86400000
seed() {
  cat >"$(store)" <<JSON
{"schema_version": 1, "data": {"next_id": 4, "notes": [
  {"id": 1, "text": "Groceries\noat milk, eggs, sourdough, lemons", "created_at_ms": $(( now_ms - 6 * day ))},
  {"id": 2, "text": "Book club\nFinish chapters 9 to 12 before Thursday", "created_at_ms": $(( now_ms - 2 * day ))},
  {"id": 3, "text": "Ideas for the weekend\nCoast walk if the weather holds, otherwise the museum", "created_at_ms": $now_ms}
]}}
JSON
}

# First launch creates the container and an empty store.
sim_first_launch "$BUNDLE_ID"
rm -f "$(store)"
sim_shoot "$OUT" 1-empty "$BUNDLE_ID"

seed
sim_shoot "$OUT" 2-notes "$BUNDLE_ID"

sim_text_size accessibility-extra-extra-extra-large
sim_shoot "$OUT" 3-notes-largest-text "$BUNDLE_ID"
sim_text_size large

# A store from a newer build: the app must offer recovery, not overwrite it.
echo '{"schema_version": 99, "data": {}}' >"$(store)"
sim_shoot "$OUT" 4-cannot-open "$BUNDLE_ID"
