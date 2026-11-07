#!/usr/bin/env bash
# Shared helpers for the simulator screenshot scripts (ios-screenshots.sh for
# the forge init starter, pocket-library-screenshots.sh for Pocket Library).
# Source it; it defines functions only.
#
# The simulator is a review aid, not part of the supported workflow (apps are
# built with xtool on Linux and run on an iPhone), and its screenshots are
# not device results.

# Boots the first available iPhone with "Pro" in its name (else any iPhone),
# fixes the status bar and installs <App.app>. Sets SIM_DEVICE.
sim_boot_and_install() { # app-path
  SIM_DEVICE="$(xcrun simctl list devices available -j | python3 -c '
import json, sys
devices = [d for runtime, ds in json.load(sys.stdin)["devices"].items()
           if "iOS" in runtime for d in ds if d["name"].startswith("iPhone")]
pro = [d for d in devices if "Pro" in d["name"] and "Max" not in d["name"]]
print((pro or devices)[0]["udid"])
')"
  echo "[screenshots] Using simulator $SIM_DEVICE"
  xcrun simctl boot "$SIM_DEVICE" 2>/dev/null || true
  xcrun simctl bootstatus "$SIM_DEVICE" -b >/dev/null
  xcrun simctl status_bar "$SIM_DEVICE" override --time 9:41 --batteryState charged --batteryLevel 100 \
    --cellularBars 4 --wifiBars 3 >/dev/null 2>&1 || true
  xcrun simctl install "$SIM_DEVICE" "$1"
}

# The app's Application Support folder in the simulator.
sim_app_support() { # bundle-id
  local data
  data="$(xcrun simctl get_app_container "$SIM_DEVICE" "$1" data)"
  echo "$data/Library/Application Support"
}

# Launches once and quits, so the app's container and empty store exist.
sim_first_launch() { # bundle-id
  xcrun simctl launch "$SIM_DEVICE" "$1" >/dev/null
  sleep 4
  xcrun simctl terminate "$SIM_DEVICE" "$1" >/dev/null 2>&1 || true
}

# <out>/<name>-light.png and -dark.png, relaunching the app for each.
# Extra arguments are passed to the app (for example -selectedTab library).
sim_shoot() { # out-dir name bundle-id [launch args...]
  local out="$1" name="$2" bundle="$3" appearance
  shift 3
  for appearance in light dark; do
    xcrun simctl ui "$SIM_DEVICE" appearance "$appearance"
    xcrun simctl terminate "$SIM_DEVICE" "$bundle" >/dev/null 2>&1 || true
    xcrun simctl launch "$SIM_DEVICE" "$bundle" "$@" >/dev/null
    sleep 4
    xcrun simctl io "$SIM_DEVICE" screenshot "$out/$name-$appearance.png" >/dev/null
    echo "[screenshots] $out/$name-$appearance.png"
  done
}

sim_text_size() { # large | accessibility-extra-extra-extra-large
  xcrun simctl ui "$SIM_DEVICE" content_size "$1"
}
