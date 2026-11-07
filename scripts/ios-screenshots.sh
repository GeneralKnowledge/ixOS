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

store() {
  local data
  data="$(xcrun simctl get_app_container "$DEVICE" "$BUNDLE_ID" data)"
  echo "$data/Library/Application Support/$APP_NAME/notes.json"
}

now_ms=$(( $(date +%s) * 1000 ))
day=86400000
seed() {
  cat >"$(store)" <<EOF
{"schema_version": 1, "data": {"next_id": 4, "notes": [
  {"id": 1, "text": "Groceries\noat milk, eggs, sourdough, lemons", "created_at_ms": $(( now_ms - 6 * day ))},
  {"id": 2, "text": "Book club\nFinish chapters 9 to 12 before Thursday", "created_at_ms": $(( now_ms - 2 * day ))},
  {"id": 3, "text": "Ideas for the weekend\nCoast walk if the weather holds, otherwise the museum", "created_at_ms": $now_ms}
]}}
EOF
}

shoot() { # name
  for appearance in light dark; do
    xcrun simctl ui "$DEVICE" appearance "$appearance"
    xcrun simctl terminate "$DEVICE" "$BUNDLE_ID" >/dev/null 2>&1 || true
    xcrun simctl launch "$DEVICE" "$BUNDLE_ID" >/dev/null
    sleep 4
    xcrun simctl io "$DEVICE" screenshot "$OUT/$1-$appearance.png" >/dev/null
    echo "[screenshots] $OUT/$1-$appearance.png"
  done
}

# First launch creates the container and an empty store.
xcrun simctl launch "$DEVICE" "$BUNDLE_ID" >/dev/null
sleep 4
xcrun simctl terminate "$DEVICE" "$BUNDLE_ID" >/dev/null 2>&1 || true
rm -f "$(store)"
shoot 1-empty

seed
shoot 2-notes

xcrun simctl ui "$DEVICE" content_size accessibility-extra-extra-extra-large
shoot 3-notes-largest-text
xcrun simctl ui "$DEVICE" content_size large

# A store from a newer build: the app must offer recovery, not overwrite it.
echo '{"schema_version": 99, "data": {}}' >"$(store)"
shoot 4-cannot-open
