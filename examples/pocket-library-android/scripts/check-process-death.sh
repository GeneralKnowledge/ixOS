#!/usr/bin/env bash
# Plan milestone A6's "done when", on an emulator or phone with the debug build installed: an
# EPUB import survives the app being backgrounded and its process being killed, and the library
# is still there after the process dies (PL-1). Uses a throwaway library, never the person's.
#
#   ./gradlew :app:installDebug && ./scripts/check-process-death.sh
set -euo pipefail

PKG=org.forgemobile.pocketlibrary
ACTIVITY="$PKG/.MainActivity"
TITLE="The Lighthouse Keeper"

pid() { adb shell pidof "$PKG" 2>/dev/null | tr -d '\r' || true; }

# Whether the screen shows $TITLE now.
shows_book() {
    adb shell uiautomator dump /sdcard/pocket-library-ui.xml >/dev/null 2>&1 || return 1
    adb shell cat /sdcard/pocket-library-ui.xml 2>/dev/null | grep -q "$TITLE"
}

await_book() {
    local seconds=$1
    for _ in $(seq 1 "$seconds"); do
        if shows_book; then return 0; fi
        sleep 1
    done
    return 1
}

relaunch_keeping_data() {
    adb shell am start -W -n "$ACTIVITY" --ez forge.temporaryData true --ez forge.keepTestData true >/dev/null
}

echo "1. A throwaway library whose sample-book import waits 90 s before it starts (long enough that a slow runner still kills it first)"
adb shell am force-stop "$PKG"
adb shell am start -W -n "$ACTIVITY" \
    --ez forge.temporaryData true --ez pocketLibrary.sampleBook true --ei pocketLibrary.holdImportSeconds 90 >/dev/null
sleep 6
if shows_book; then
    echo "FAIL: the import didn't wait, so this run proves nothing" >&2
    exit 1
fi

echo "2. Leave the app, then end its process with the import still to do"
adb shell input keyevent KEYCODE_HOME
sleep 2
before=$(pid)
if [ -z "$before" ]; then
    echo "FAIL: the app's process ended on its own before the test could end it" >&2
    exit 1
fi
adb shell am kill "$PKG"
sleep 2
if [ "$(pid)" = "$before" ]; then
    # `am kill` ends only processes the system counts as idle, and one running a job isn't.
    # End it the way the low-memory killer does (a debug build lets run-as do it).
    echo "   am kill left the process (it is running a job); killing it with SIGKILL"
    adb shell run-as "$PKG" kill -9 "$before" || true
    sleep 2
fi
if [ "$(pid)" = "$before" ]; then
    echo "FAIL: the app's process ($before) is still running" >&2
    exit 1
fi
echo "   process $before ended"

echo "3. Open the app again on the same throwaway library: the import finishes"
relaunch_keeping_data
if ! await_book 90; then
    echo "FAIL: '$TITLE' never arrived after the process was killed" >&2
    adb logcat -d | grep -E "WM-|PocketLibrary|forge|AndroidRuntime" | tail -80 || true
    exit 1
fi
echo "   '$TITLE' is in the library"

echo "4. PL-1 after a process death: stop the app and open it again"
adb shell am force-stop "$PKG"
relaunch_keeping_data
if ! await_book 30; then
    echo "FAIL: '$TITLE' was gone after the app was stopped" >&2
    exit 1
fi
echo "PASS: the import survived the process being killed, and the library survived a restart"
adb shell am force-stop "$PKG"
