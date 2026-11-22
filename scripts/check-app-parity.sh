#!/usr/bin/env bash
# Checks that every example app keeps up with the framework (docs/app-parity.md).
#
# Each rule is a plain fact about an app's files: it hosts its Rust core with
# ForgeHost instead of its own lifecycle code, sends iOS requests through the
# URLSession `http` service, has a smoke test, is reachable from Device Lab,
# and so on. Apps that lag today are listed in docs/app-parity.toml with the
# reason and the plan wave that catches them up. The check fails on a lag
# that isn't listed (a new copy can't slip in) and on a listed lag that has
# been fixed (so the list only shrinks). Each entry has a status: missing
# (the app should use the framework and doesn't yet; the default), deferred
# (known work postponed on purpose) or not-applicable (the app has a real
# reason for its own plumbing). It also checks that
# docs/app-parity.md, the generated table, is up to date.
#
# Usage: scripts/check-app-parity.sh           check (make validate, CI)
#        scripts/check-app-parity.sh --write   rewrite docs/app-parity.md
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
exec python3 -I - "$ROOT" "$@" <<'PY'
import pathlib
import re
import sys
import tomllib

root = pathlib.Path(sys.argv[1])
write = "--write" in sys.argv[2:]
examples = root / "examples"
lag_file = root / "docs" / "app-parity.toml"
page = root / "docs" / "app-parity.md"


def read(path):
    try:
        return path.read_text(encoding="utf-8")
    except (FileNotFoundError, UnicodeDecodeError):
        return ""


def swift_sources(app):
    """The app's own Swift, never generated bindings."""
    return [
        p
        for p in sorted((app / "Sources").rglob("*.swift"))
        if "Generated" not in p.parts
    ]


def rust_sources(app):
    src = app / "rust" / "src"
    return sorted(src.rglob("*.rs")) if src.is_dir() else []


def native_services(app):
    """The optional services named in the crate's `native_services!(...)` call
    (not in a comment that mentions it)."""
    for path in rust_sources(app):
        for line in read(path).splitlines():
            if line.lstrip().startswith("//"):
                continue
            m = re.search(r"native_services!\(([^)]*)\)", line)
            if m:
                return {s.strip() for s in m.group(1).split(",") if s.strip()}
    return None


def hosted_in_device_lab():
    text = read(examples / "device-lab" / "Package.swift")
    return set(re.findall(r'\.package\(path: "\.\./([^"/]+)"\)', text))


# --- the apps -------------------------------------------------------------

# An app is an example with a Swift package. Device Lab is the host, so the
# rules about being hosted don't apply to it; the ForgeUI showcase is a plain
# SwiftUI app with no Rust, so the Rust rules don't apply to it.
apps = sorted(
    p.name for p in examples.iterdir() if (p / "Package.swift").is_file()
)
HOST = "device-lab"
NO_RUST = {p for p in apps if not (examples / p / "rust" / "Cargo.toml").is_file()}
hosted = hosted_in_device_lab()


# --- the rules ------------------------------------------------------------
# Each returns None when the rule doesn't apply to the app, else
# (passes, detail).


def rule_forgekit(app):
    text = read(app / "Package.swift")
    ok = bool(re.search(r'(?i)forgekit"\)|github\.com/GeneralKnowledge/ForgeKit', text))
    return ok, "" if ok else "Package.swift doesn't depend on ForgeKit"


def rule_forge_host(app):
    if app.name in NO_RUST:
        return None
    own = [p.name for p in swift_sources(app) if p.name == "AppLifecycle.swift"]
    runtime = any("export_runtime!" in read(p) for p in rust_sources(app))
    if own:
        return False, "keeps its own AppLifecycle.swift"
    if not runtime:
        return False, "Rust core isn't on the forge-app runtime (no export_runtime!)"
    return True, ""


def rule_appkit_alerts(app):
    own = [p for p in swift_sources(app) if p.name == "ErrorAlert.swift"]
    return (False, "keeps its own ErrorAlert.swift") if own else (True, "")


def rule_forgeui_appearance(app):
    own = [p for p in swift_sources(app) if p.name == "LaunchAppearance.swift"]
    return (False, "keeps its own LaunchAppearance.swift") if own else (True, "")


# Haptics come from ForgeAppKit's ForgeFeedback (play(.success) in the
# model, .forgeFeedback in the view), not a hand-rolled saveCount counter
# (Ice Cubes audit, proposal 2; 2026-11-17).
def rule_feedback(app):
    # Outcome haptics (done, failed) go through ForgeFeedback. A haptic that
    # is part of the app's own interaction (a step tap, a quiz answer, a light
    # tap on send) stays the app's: only the success/error outcomes count.
    outcome = re.compile(r"\bsaveCount\b|\.sensoryFeedback\(\s*\.(success|error)\s*,")
    bad = [p.name for p in swift_sources(app) if outcome.search(read(p))]
    if bad:
        return False, "plays outcome haptics by hand (saveCount, .sensoryFeedback(.success/.error)); use ForgeFeedback: " + ", ".join(bad)
    return True, ""


def rule_urlsession(app):
    if app.name in NO_RUST:
        return None
    # ureq is fine in command-line binaries (src/bin) and tests.
    uses_ureq = any(
        "UreqClient" in read(p) for p in rust_sources(app) if "bin" not in p.parts
    )
    if not uses_ureq:
        return None
    services = native_services(app) or set()
    if "http" in services:
        return True, ""
    return False, "iOS requests go through ureq, not the `http` service"


def rule_location(app):
    own = [p for p in swift_sources(app) if "CLLocationManager" in read(p)]
    if not own:
        return None
    services = native_services(app) or set()
    if "location" in services:
        return True, ""
    return False, f"own Core Location code ({own[0].name}), not the `location` service"


# Network changes come from the `connectivity` service (Rust watches it),
# not an NWPathMonitor of the app's own (Commonplace's offline refresh,
# 2026-11-17).
def rule_connectivity(app):
    own = [p for p in swift_sources(app) if "NWPathMonitor" in read(p)]
    if not own:
        return None
    return False, f"own network monitor ({own[0].name}), not the `connectivity` service"


# Rust work cancelled from Swift goes through ForgeKit's withRustCancellation,
# not a Cancellation wired to withTaskCancellationHandler by hand (framework
# candidates PR A, 2026-11-17). withTaskCancellationHandler on its own (an
# audio continuation, a URLSession call) is fine.
def rule_rust_cancellation(app):
    bad = [
        p.name
        for p in swift_sources(app)
        if re.search(r"(?<![\w.])Cancellation\(\)", read(p)) and "withTaskCancellationHandler" in read(p)
    ]
    if bad:
        return False, "wires a Cancellation to withTaskCancellationHandler by hand; use withRustCancellation: " + ", ".join(bad)
    return True, ""


# Swift logs through ForgeLog, so records reach `forge device logs` and crash
# reports; print and NSLog only reach a debugger (framework candidates PR A,
# 2026-11-17). Exempt: the command-line launchers (`*Launcher/AppMain.swift`),
# which print to the terminal that runs them.
SWIFT_PRINT = re.compile(r"(?<![\w.])(?:print|NSLog|debugPrint)\(")


def rule_swift_logging(app):
    bad = []
    for path in swift_sources(app):
        if path.name == "AppMain.swift" and path.parent.name.endswith("Launcher"):
            continue
        in_string = False
        for i, line in enumerate(read(path).splitlines()):
            # Lines inside a multi-line string literal are text, not Swift
            # (Python Lab's sample scripts call Python's print).
            quotes = line.count('"""') % 2 == 1
            if not in_string and not quotes and SWIFT_PRINT.search(line.split("//", 1)[0]):
                bad.append(f"{path.name}:{i + 1}")
            if quotes:
                in_string = not in_string
    if bad:
        return False, "logs with print or NSLog; use ForgeLog: " + ", ".join(bad)
    return True, ""


def rule_requirements(app):
    if app.name == HOST or app.name in NO_RUST:
        return None
    path = app / "requirements.toml"
    if not path.is_file():
        return False, "no requirements.toml"
    data = tomllib.loads(read(path))
    reqs = data.get("requirement", [])
    if not reqs:
        return False, "requirements.toml lists no requirements"
    bare = [r.get("id", "?") for r in reqs if not r.get("twin") and not r.get("device")]
    if bare:
        return False, "no test for " + ", ".join(bare)
    return True, ""


def rule_device_checks(app):
    path = app / "requirements.toml"
    if app.name == HOST or not path.is_file():
        return None
    reqs = tomllib.loads(read(path)).get("requirement", [])
    missing = [r.get("id", "?") for r in reqs if not r.get("device")]
    if missing:
        return False, f"{len(missing)} of {len(reqs)} requirements have no device scenario"
    return True, ""


def rule_ui_test(app):
    if app.name == HOST:
        return None
    ok = any((app / "uitests").glob("*.toml"))
    return ok, "" if ok else "no UI scenario in uitests/"


def rule_device_lab(app):
    if app.name == HOST:
        return None
    ok = app.name in hosted
    return ok, "" if ok else "not on Device Lab's Apps screen"


def rule_device_family(app):
    # xtool marks every app for iPhone and iPad unless told otherwise, so
    # each app says where it runs (docs/app-config.md).
    toml = app / "forge.toml"
    if toml.is_file():
        ok = bool(re.search(r"(?m)^devices\s*=", read(toml)))
        return ok, "" if ok else "forge.toml [ios] doesn't set devices"
    ok = "<key>UIDeviceFamily</key>" in read(app / "Info.plist")
    return ok, "" if ok else "Info.plist doesn't set UIDeviceFamily"


def stack_alert_problems(text):
    """App-wide alerts attached to the first screen of a navigation stack
    that pushes other screens. iOS shows such an alert behind the pushed
    screen (FieldKit's Stop, 2026-11-16). An alert bound to the model (shared
    by every screen) belongs on the stack itself, on the tabs, or gated on
    the stack's path being empty."""
    lines = text.splitlines()
    found = []
    open_stacks = []  # (indent, first line, pushes)
    for i, line in enumerate(lines):
        indent = len(line) - len(line.lstrip())
        stripped = line.strip()
        while open_stacks and stripped.startswith("}") and indent == open_stacks[-1][0]:
            open_stacks.pop()
        if re.search(r"\bNavigationStack\s*(\(|\{)", line):
            end = next(
                (j for j in range(i + 1, len(lines))
                 if lines[j].startswith(" " * indent + "}")
                 and len(lines[j]) - len(lines[j].lstrip()) == indent),
                len(lines),
            )
            block = "\n".join(lines[i:end])
            pushes = bool(re.search(r"navigationDestination|NavigationLink", block))
            open_stacks.append((indent, i, pushes))
            continue
        if not open_stacks or not open_stacks[-1][2]:
            continue
        if not re.match(r"\.(forgeAlert|alert|confirmationDialog)\(", stripped):
            continue
        binding = " ".join(l.strip() for l in lines[i : i + 4])
        binding = binding.split(") {")[0]
        shared = re.search(r"\$model\.|Bindable\(model\)|model\.(alert|actionError|errorMessage)", binding)
        if shared and "path" not in binding:
            found.append(i + 1)
    return found


def rule_alerts_on_top(app):
    bad = []
    for path in swift_sources(app):
        for line in stack_alert_problems(read(path)):
            bad.append(f"{path.name}:{line}")
    if bad:
        return False, "app-wide alert on a screen that pushes others (shows behind them): " + ", ".join(bad)
    return True, ""


# A model-wide alert can't show while a sheet is up: iOS presents it from the
# screen under the sheet, so a failure from inside the sheet is never seen
# (Commonplace's Settings, Relay's sheets and the starter, friction log A39).
# A screen that presents sheets gates the model's alert while one is open,
# and the sheet shows the alerts itself. Each var or func is looked at on its
# own, so an alert inside a sheet's own content is fine.
SHARED_ALERT = re.compile(r"\$model\.|Bindable\(model\)|model\.(alert|actionError|errorMessage)")
MEMBER = re.compile(
    r"(?:(?:private|fileprivate|public|internal|static|@ViewBuilder|@MainActor)\s+)*"
    r"(?:var|func|struct|extension|class|init|enum)\b"
)


def sheet_alert_problems(text):
    lines = text.splitlines()
    starts = [
        i for i, line in enumerate(lines)
        if len(line) - len(line.lstrip()) <= 4 and MEMBER.match(line.strip())
    ]
    found = []
    for begin, end in zip([0] + starts, starts + [len(lines)]):
        member = lines[begin:end]
        sheets = set()
        for line in member:
            for m in re.finditer(r"\.(?:sheet|fullScreenCover)\(\s*(?:isPresented|item):\s*\$([\w.]+)", line):
                sheets.add(m.group(1).split(".")[-1])
        # forgeRename and forgeShareFile are sheets too; forgeRename's
        # binding follows the title, often on a later line.
        for m in re.finditer(r"\.forgeRename\([^{]*?\b(?:isPresented|item):\s*\$([\w.]+)", "\n".join(member)):
            sheets.add(m.group(1).split(".")[-1])
        for m in re.finditer(r"\.forgeShareFile\(\s*\$([\w.]+)", "\n".join(member)):
            sheets.add(m.group(1).split(".")[-1])
        if not sheets:
            continue
        for j, line in enumerate(member):
            if not re.match(r"\.(forgeAlert|alert)\(", line.strip()):
                continue
            binding = " ".join(l.strip() for l in member[j : j + 4]).split(") {")[0]
            if not SHARED_ALERT.search(binding):
                continue
            if any(re.search(r"\b" + re.escape(name) + r"\b", binding) for name in sheets):
                continue
            found.append(begin + j + 1)
    return found


def rule_alerts_over_sheets(app):
    bad = []
    for path in swift_sources(app):
        for line in sheet_alert_problems(read(path)):
            bad.append(f"{path.name}:{line}")
    if bad:
        return False, "model alert on a screen with sheets, not held back while one is up: " + ", ".join(bad)
    return True, ""


# A picked file is copied with ForgeAppKit's ForgePickedFile, which copies
# off the main thread and deletes the copy when released. Relay froze for
# seconds copying a large video on the main thread (friction log A40), and
# Chat copied picked documents the same way. Reading a picked file in place
# inside its security scope, with no copy, is fine.
def picked_copy_problems(text):
    if "startAccessingSecurityScopedResource" not in text:
        return []
    return [
        i + 1
        for i, line in enumerate(text.splitlines())
        if re.search(r"\.copyItem\(", line.split("//", 1)[0])
    ]


def rule_picked_files(app):
    bad = []
    for path in swift_sources(app):
        for line in picked_copy_problems(read(path)):
            bad.append(f"{path.name}:{line}")
    if bad:
        return False, "copies a picked file itself; use ForgePickedFile.copy(of:): " + ", ".join(bad)
    return True, ""


# A user's action that fails must say so (FieldKit's Rename, 2026-11-16).
# `try?` throws the reason away, so it may wrap reads and clean-up but not a
# call that changes something; a catch-all `catch` that does nothing hides
# every failure (catching CancellationError alone is fine).
CHANGING_CALL = re.compile(
    r"try\?\s*(?:await\s+)?(?!FileManager)[\w.?]*?\b"
    r"(rename|remove|delete|move|save|add|set|update|create|record|start|stop|send|import|export|pair|forget)\w*\("
)


def dropped_error_problems(text):
    lines = text.splitlines()
    found = []
    for i, line in enumerate(lines):
        code = line.split("//", 1)[0]
        if CHANGING_CALL.search(code):
            found.append(i + 1)
            continue
        m = re.search(r"\bcatch\s*\{(.*)$", code)
        if not m:
            continue
        rest = m.group(1)
        if rest.strip().startswith("}"):
            found.append(i + 1)
            continue
        if rest.strip():
            continue
        body = []
        for later in lines[i + 1 :]:
            stripped = later.strip()
            if stripped.startswith("}"):
                break
            if stripped and not stripped.startswith("//"):
                body.append(stripped)
        if not body:
            found.append(i + 1)
    return found


def rule_errors_shown(app):
    bad = []
    for path in swift_sources(app):
        for line in dropped_error_problems(read(path)):
            bad.append(f"{path.name}:{line}")
    if bad:
        return False, "a failed action is dropped (`try?` on a change, or an empty catch): " + ", ".join(bad)
    return True, ""


# Raw orange, yellow or green text fails contrast on a light background (the
# friction audit, 2026-11-16; green added by the ForgeUI coverage audit E3,
# 2026-11-18); ForgeUI's tones pass in light and dark:
# `.forgeForeground(.warning)`, `.forgeForeground(.success)`. Fills and tints
# may stay any colour.
RAW_TONE_TEXT = re.compile(r"\.foreground(?:Style|Color)\(\s*(?:Color)?\.(orange|yellow|green)\s*\)")


# Error text in raw red (framework candidates PR A, 2026-11-17): the
# message a Text or Label shows is an error, problem, failure or reason, and
# the same view is styled `.red`. Red that marks a state (recording) is left
# alone: only these error presentations count.
RAW_RED = re.compile(r"\.foreground(?:Style|Color)\(\s*(?:Color)?\.red\s*\)")
ERROR_TEXT = re.compile(r"\b(?:Text|Label)\(\s*(?:\w+\.)*(?:error|problem|failure|reason)\b")


def raw_tone_problems(text):
    lines = text.splitlines()
    found = []
    for i, line in enumerate(lines):
        code = line.split("//", 1)[0]
        if RAW_TONE_TEXT.search(code):
            found.append(i + 1)
            continue
        if not RAW_RED.search(code):
            continue
        # The view this modifier styles: this line, or the lines above it up
        # to the one that starts the view.
        for j in range(i, max(-1, i - 4), -1):
            start = lines[j].split("//", 1)[0]
            if ERROR_TEXT.search(start):
                found.append(i + 1)
                break
            if j < i and not start.strip().startswith("."):
                break
    return found


def rule_toned_text(app):
    bad = []
    for path in swift_sources(app):
        for line in raw_tone_problems(read(path)):
            bad.append(f"{path.name}:{line}")
    if bad:
        return False, "orange, yellow, green or red error text; use .forgeForeground(.warning), (.success) or (.danger): " + ", ".join(bad)
    return True, ""


# Empty, no-results, loading and failed screens through ForgeUI's
# `ForgeStateView` (framework candidates PR B, 2026-11-17), which keeps the
# states apart, gives every action an identifier and uses the system's own
# no-results view. Applies to every app on ForgeKit, ForgeUI or not, so
# leaving ForgeUI out isn't a way round it (ForgeUI coverage audit E1).
def rule_state_views(app):
    bad = []
    for path in swift_sources(app):
        for i, line in enumerate(read(path).splitlines()):
            if re.search(r"\bContentUnavailableView\b", line.split("//", 1)[0]):
                bad.append(f"{path.name}:{i + 1}")
    if bad:
        return False, "builds state screens with ContentUnavailableView; use ForgeStateView: " + ", ".join(bad)
    return True, ""


# A sheet kept open (while saving, or holding unsaved input) through
# ForgeUI's `forgeSwipeGuard` or `forgeFormToolbar`. SwiftUI's
# `interactiveDismissDisabled` didn't stop the swipe on iOS 26.3 when applied
# inside the sheet's NavigationStack (#326 phone check, 2026-11-18: the
# showcase task form and Pocket Library's Add Book both closed with text
# typed). Applies to every app, ForgeUI or not: the fix is in ForgeUI.
def rule_swipe_guard(app):
    bad = []
    for path in swift_sources(app):
        for i, line in enumerate(read(path).splitlines()):
            if re.search(r"\.interactiveDismissDisabled\s*\(", line.split("//", 1)[0]):
                bad.append(f"{path.name}:{i + 1}")
    if bad:
        return False, "keeps a sheet open with interactiveDismissDisabled, which iOS 26.3 ignores; use forgeSwipeGuard: " + ", ".join(bad)
    return True, ""


# The app's look comes from a ForgeUI theme set once at its root
# (`.forgeTheme(...)`), which also tints plain SwiftUI controls with the
# app's accent (ForgeUI coverage audit E2, 2026-11-18).
def rule_forge_theme(app):
    if any(".forgeTheme(" in read(p) for p in swift_sources(app)):
        return True, ""
    return False, "sets no ForgeUI theme; add .forgeTheme(...) at the root view"


# A screen that is still opening shows ForgeUI's `ForgeStateView.loading()`,
# not a bare `ProgressView()` as the whole screen: the same size and place
# in every app, and room for a label (ForgeUI coverage audit E2).
LOADING_CASE = re.compile(r"^\s*case\s+\.(?:loading|opening|starting)\b[^:]*:\s*$")
BARE_SPINNER = re.compile(r"^\s*ProgressView\(\)\s*$")


def rule_root_loading(app):
    bad = []
    for path in swift_sources(app):
        lines = read(path).splitlines()
        for i, line in enumerate(lines[:-1]):
            if not (LOADING_CASE.match(line) and BARE_SPINNER.match(lines[i + 1].split("//", 1)[0])):
                continue
            # A spinner sized or placed by the lines after it is part of a
            # screen (a section still loading), not the whole screen.
            after = lines[i + 2].lstrip() if i + 2 < len(lines) else ""
            if not after.startswith("."):
                bad.append(f"{path.name}:{i + 2}")
    if bad:
        return False, "a loading screen is a bare ProgressView(); use ForgeStateView.loading(): " + ", ".join(bad)
    return True, ""


def rule_web_images(app):
    bad = []
    for path in swift_sources(app):
        for i, line in enumerate(read(path).splitlines()):
            if re.search(r"\bAsyncImage\s*\(", line.split("//", 1)[0]):
                bad.append(f"{path.name}:{i + 1}")
    if bad:
        return False, "pictures from the web with AsyncImage; use ForgeRemoteImage: " + ", ".join(bad)
    return True, ""


# Coming back from the background through ForgeHost (`onResume`,
# `onBackground`, `lastResume`), decided in the model and tested there, not
# in a view watching SwiftUI's scenePhase (docs/research/coming-back.md,
# 2026-11-17). Four apps had each fixed the same bug that way.
def rule_resume(app):
    bad = []
    for path in swift_sources(app):
        for i, line in enumerate(read(path).splitlines()):
            if re.search(r"\bscenePhase\b", line.split("//", 1)[0]):
                bad.append(f"{path.name}:{i + 1}")
    if bad:
        return False, "watches scenePhase in a view; follow the host with ForgeHost.onResume in the model: " + ", ".join(bad)
    return True, ""


# The lock screen, headphone buttons and the system's audio rules (calls,
# Siri, unplugged headphones) through the `ForgeNowPlaying` helper, not by
# hand in each player (docs/research/framework-candidates.md, 2026-11-18). Read
# Aloud's hand-written copy had none of the system rules.
NOW_PLAYING = re.compile(
    r"\bMPNowPlayingInfoCenter\b|\bMPRemoteCommandCenter\b"
    r"|\binterruptionNotification\b|\brouteChangeNotification\b"
)


def rule_now_playing(app):
    bad = []
    for path in swift_sources(app):
        for i, line in enumerate(read(path).splitlines()):
            if NOW_PLAYING.search(line.split("//", 1)[0]):
                bad.append(f"{path.name}:{i + 1}")
    if bad:
        return False, "lock screen or audio interruptions by hand; use ForgeNowPlaying: " + ", ".join(bad)
    return True, ""


# A bar floating over a screen's content (a mini-player, an undo banner)
# through ForgeUI's `forgeBar`, which gives every scroll view below it room,
# pushed screens included. Hand-made versions measured the bar themselves
# and padded each screen with clear space; Soundscape's hid an album's last
# song and Commonplace's banner covered the last article
# (docs/research/screen-space.md, 2026-11-21). Bottom bars only: a header
# that content deliberately slides under (Weather's condensed header, which
# cards pin beneath) needs no room and is not a forgeBar.
def closure_after(lines, i):
    """The text of the first `{ ... }` block that opens on line i or later
    (comments already stripped), so a check sees a modifier's whole body."""
    depth, out, started = 0, [], False
    for line in lines[i:]:
        for ch in line:
            if ch == "{":
                depth += 1
                started = True
            elif ch == "}":
                depth -= 1
        out.append(line)
        if started and depth <= 0:
            break
    return " ".join(out)


def modifier_args(lines, i, name):
    """The text between `name(` on line `i` and its closing parenthesis,
    however many lines the arguments take."""
    text = "\n".join(lines[i:])
    start = text.find(name + "(")
    if start < 0:
        return ""
    depth = 0
    for j in range(start + len(name), len(text)):
        if text[j] == "(":
            depth += 1
        elif text[j] == ")":
            depth -= 1
            if depth == 0:
                return text[start + len(name) + 1 : j]
    return ""


BOTTOM_ALIGNMENT = re.compile(r"^\s*alignment:\s*\.bottom\s*$")
BOTTOM_EDGE = re.compile(r"^\s*edge:\s*\.bottom\b")


def rule_floating_bars(app):
    bad = []
    for path in swift_sources(app):
        lines = [l.split("//", 1)[0] for l in read(path).splitlines()]
        for i, line in enumerate(lines):
            if ".overlay(" in line:
                if BOTTOM_ALIGNMENT.search(modifier_args(lines, i, ".overlay")) and "onGeometryChange" in closure_after(lines, i):
                    bad.append(f"{path.name}:{i + 1}")
            elif ".safeAreaInset(" in line:
                if BOTTOM_EDGE.search(modifier_args(lines, i, ".safeAreaInset")) and "Color.clear" in closure_after(lines, i):
                    bad.append(f"{path.name}:{i + 1}")
    if bad:
        return False, "floats a bar by hand; use ForgeUI's forgeBar: " + ", ".join(bad)
    return True, ""


# A notice in a screen that still works ("offline", "the map can't update")
# is ForgeUI's ForgeNotice. Built by hand, it is a Label with a problem
# symbol, made small and grey or toned: each app picked its own size,
# colour and place for the same message (app UI review, November 2026).
PROBLEM_SYMBOL = re.compile(r'systemImage:\s*"(exclamationmark|wifi|[^"]*\.slash\b|[^"]*\.badge\.exclamationmark|[^"]*exclamationmark\.(triangle|circle|icloud))[^"]*"')
SMALL_FONT = re.compile(r"\.font\(\.(footnote|callout|caption)\)")
QUIET_COLOUR = re.compile(r"\.foregroundStyle\(\.secondary\)|\.forgeForeground\(")


def swift_code_mask(text, keep_strings=False):
    """`text` with comments blanked and string-literal contents replaced by
    `x`, the same length and with the same line breaks, so parentheses and
    `//` inside a string ("(\\(reason))", a URL) don't count as code.
    Interpolated code inside a string stays code. With `keep_strings`, only
    comments are blanked."""
    out = list(text)
    stack = []  # "s" (string), "m" (multi-line string) or an int (paren depth inside \( ... ))
    i, n = 0, len(text)
    while i < n:
        c = text[i]
        top = stack[-1] if stack else None
        if top in ("s", "m"):
            if c == "\\" and i + 1 < n and text[i + 1] == "(":
                out[i] = out[i + 1] = "x"
                stack.append(0)
                i += 2
                continue
            if c == "\\":
                out[i] = "x"
                if i + 1 < n and text[i + 1] != "\n":
                    out[i + 1] = "x"
                i += 2
                continue
            if top == "m" and text.startswith('"""', i):
                stack.pop()
                i += 3
                continue
            if top == "s" and c == '"':
                stack.pop()
                i += 1
                continue
            if c != "\n":
                out[i] = "x"
            i += 1
            continue
        # code (top level or inside an interpolation)
        if text.startswith("//", i):
            while i < n and text[i] != "\n":
                out[i] = " "
                i += 1
            continue
        if text.startswith("/*", i):
            # Swift block comments nest.
            depth = 0
            while i < n:
                if text.startswith("/*", i):
                    depth += 1
                    out[i] = out[i + 1] = " "
                    i += 2
                elif text.startswith("*/", i):
                    depth -= 1
                    out[i] = out[i + 1] = " "
                    i += 2
                    if depth == 0:
                        break
                else:
                    if text[i] != "\n":
                        out[i] = " "
                    i += 1
            continue
        if text.startswith('"""', i):
            stack.append("m")
            i += 3
            continue
        if c == '"':
            stack.append("s")
        elif isinstance(top, int):
            if c == "(":
                stack[-1] += 1
            elif c == ")":
                if top == 0:
                    stack.pop()
                    out[i] = "x"
                else:
                    stack[-1] -= 1
        i += 1
    if keep_strings:
        return "".join(t if o == "x" else o for t, o in zip(text, out))
    return "".join(out)


def call_and_chain(plain, code, start):
    """For a call whose `(` is at `start` in `code` (the source with strings
    and comments masked): its arguments from `plain` (comments blanked,
    strings intact) and the modifiers chained
    after its closing parenthesis, one per following line starting with `.`."""
    depth = 0
    for j in range(start, len(code)):
        if code[j] == "(":
            depth += 1
        elif code[j] == ")":
            depth -= 1
            if depth == 0:
                break
    else:
        return "", ""
    args = plain[start + 1 : j]
    rest = code[j + 1 :].split("\n")
    chain = [rest[0]]
    for line in rest[1:]:
        if not line.strip().startswith("."):
            break
        chain.append(line)
    return args, "\n".join(chain)


def rule_notices(app):
    bad = []
    for path in swift_sources(app):
        text = read(path)
        code = swift_code_mask(text)
        plain = swift_code_mask(text, keep_strings=True)
        for m in re.finditer(r"\bLabel\(", code):
            args, chain = call_and_chain(plain, code, m.end() - 1)
            if PROBLEM_SYMBOL.search(args) and SMALL_FONT.search(chain) and QUIET_COLOUR.search(chain):
                bad.append(f"{path.name}:{text.count(chr(10), 0, m.start()) + 1}")
    if bad:
        return False, "builds a notice by hand; use ForgeUI's ForgeNotice: " + ", ".join(bad)
    return True, ""


# Naming something (rename a trip, a new folder) is ForgeUI's forgeRename:
# a sheet that keeps what was typed and shows why Rust refused a name under
# the field. Built by hand it was an alert with a TextField, which can show
# neither, or an app's own name sheet; six apps had one each, with their
# own identifiers (ForgeUI coverage audit C2, November 2026).
NAME_PROMPT = re.compile(r"renam|\bnam(e|ing)", re.I)
OWN_NAME_SHEET = re.compile(r"\bstruct\s+\w*(Rename|Name)Sheet\b")


def brace_block(code, start):
    """The `{ ... }` block that starts at `start` in `code`, after any
    whitespace (a trailing closure), or "" when something else comes first."""
    open_at = start
    while open_at < len(code) and code[open_at].isspace():
        open_at += 1
    if open_at >= len(code) or code[open_at] != "{":
        return ""
    depth = 0
    for j in range(open_at, len(code)):
        if code[j] == "{":
            depth += 1
        elif code[j] == "}":
            depth -= 1
            if depth == 0:
                return code[open_at : j + 1]
    return code[open_at:]


def rule_rename(app):
    bad = []
    for path in swift_sources(app):
        text = read(path)
        code = swift_code_mask(text)
        plain = swift_code_mask(text, keep_strings=True)
        line = lambda at: text.count(chr(10), 0, at) + 1
        for m in re.finditer(r"\.alert\(", code):
            args, _ = call_and_chain(plain, code, m.end() - 1)
            # The actions closure after the arguments (a Binding's own
            # closures sit inside them), or an `actions:` argument.
            close = m.end() + len(args) + 1
            # The name can be in the title or in the field ("New Folder"
            # with TextField("Name", …)); a Note alert has neither.
            actions = args + brace_block(plain, close)
            if "TextField(" in actions and NAME_PROMPT.search(actions):
                bad.append(f"{path.name}:{line(m.start())}")
        for m in OWN_NAME_SHEET.finditer(code):
            bad.append(f"{path.name}:{line(m.start())}")
    if bad:
        return False, "names things in its own alert or sheet; use ForgeUI's forgeRename: " + ", ".join(bad)
    return True, ""


# A form sheet's Cancel and confirm buttons are ForgeUI's forgeFormToolbar:
# the confirm button turns into a labelled spinner while it saves, Cancel
# waits, and the sheet can't be swiped away while it saves or holds changes.
# Seven sheets in five apps built the same toolbar by hand, each with its own
# identifiers and its own swipe guard (ForgeUI 0.2, item 1). A hand-built one
# shows as a `.confirmationAction` toolbar item that holds a ProgressView.
CONFIRM_ITEM = re.compile(r"\bToolbarItem\(\s*placement:\s*\.confirmationAction\s*\)")


def rule_form_toolbar(app):
    bad = []
    for path in swift_sources(app):
        text = read(path)
        code = swift_code_mask(text)
        for m in CONFIRM_ITEM.finditer(code):
            if "ProgressView" in brace_block(code, m.end()):
                bad.append(f"{path.name}:{text.count(chr(10), 0, m.start()) + 1}")
    if bad:
        return False, "builds a form toolbar by hand; use ForgeUI's forgeFormToolbar: " + ", ".join(bad)
    return True, ""


# A coloured tile behind a row's symbol is ForgeRow's icon tile, coloured by
# a tone or one of ForgeUI's checked accents (decision D1, ForgeUI coverage
# audit). Built by hand it was a symbol on a RoundedRectangle in a raw
# colour (`.pink`, `tint.gradient`), with its own size and corner, unchecked
# for contrast in dark mode (ForgeUI 0.2, item 2). Materials and the theme's
# own colours are surfaces, not tiles, and are left alone. Only a background
# whose modifier chain starts at `Image(systemName:)` counts, so a stack that
# merely holds a symbol is not flagged.
TILE_BACKGROUND = re.compile(r"\.background\(\s*([^,()]+(?:\([^()]*\))?[^,()]*),\s*in:\s*RoundedRectangle\b")
SURFACE = re.compile(r"Material|theme\.|\.quaternary|\.tertiary|\.secondary|\.fill\b")


def chain_root(code, dot):
    """The expression a modifier chain starts from: walks back from the `.`
    of a modifier over `.name(...)`, `.name { ... }` and `.name` links,
    skipping balanced brackets, and returns the text from the root onwards."""
    pairs = {")": "(", "]": "[", "}": "{"}
    pos = dot
    while True:
        while pos > 0 and code[pos - 1].isspace():
            pos -= 1
        while pos > 0 and code[pos - 1] in pairs:
            depth, close, open_ = 0, code[pos - 1], pairs[code[pos - 1]]
            while pos > 0:
                pos -= 1
                if code[pos] == close:
                    depth += 1
                elif code[pos] == open_:
                    depth -= 1
                    if depth == 0:
                        break
            while pos > 0 and code[pos - 1].isspace():
                pos -= 1
        while pos > 0 and (code[pos - 1].isalnum() or code[pos - 1] == "_"):
            pos -= 1
        back = pos
        while back > 0 and code[back - 1].isspace():
            back -= 1
        if back > 0 and code[back - 1] == ".":
            pos = back - 1
            continue
        return code[pos:dot].lstrip()


def rule_icon_tile(app):
    bad = []
    for path in swift_sources(app):
        text = read(path)
        code = swift_code_mask(text)
        for m in TILE_BACKGROUND.finditer(code):
            if SURFACE.search(m.group(1)):
                continue
            if re.match(r"Image\(\s*systemName:", chain_root(code, m.start())):
                bad.append(f"{path.name}:{text.count(chr(10), 0, m.start()) + 1}")
    if bad:
        return False, "draws an icon tile by hand; use ForgeRow's systemImage with a tone or an accent: " + ", ".join(bad)
    return True, ""


# Pictures (covers, photos, artwork) sit in ForgeUI's ForgeThumbnail, which
# sizes, shapes and outlines them the same everywhere and draws one
# placeholder for empty, loading and failed. Five apps clipped their own,
# with five corner radii and three placeholders (ForgeUI 0.2, item 5). This
# finds an image clipped to a fixed-radius rectangle or a circle by hand.
HAND_THUMBNAIL = re.compile(r"\.clipShape\(\s*(?:RoundedRectangle\(\s*cornerRadius:\s*[\d.]+|Circle\(\))")


def rule_thumbnail(app):
    bad = []
    for path in swift_sources(app):
        text = read(path)
        code = swift_code_mask(text)
        for m in HAND_THUMBNAIL.finditer(code):
            if re.match(r"(?:Image|ForgeRemoteImage|AsyncImage)\(", chain_root(code, m.start())):
                bad.append(f"{path.name}:{text.count(chr(10), 0, m.start()) + 1}")
    if bad:
        return False, "clips a picture by hand; put it in ForgeUI's ForgeThumbnail: " + ", ".join(bad)
    return True, ""


# A choice among several (chips, tiles, A/B) wears ForgeUI's choice style
# (`.buttonStyle(.forgeChoice(isSelected:))`, `.toggleStyle(.forgeChoice)`),
# which adds the `.isSelected` trait, a 44-point tap and a chosen look that
# isn't colour alone. Three apps built their own, each with its own colours
# and sizes (ForgeUI 0.2, item 6). An app adding the trait itself has built
# one by hand.
HAND_SELECTED = re.compile(r"\.accessibilityAddTraits\([^)\n]*\.isSelected")


def rule_choice(app):
    bad = []
    for path in swift_sources(app):
        text = read(path)
        code = swift_code_mask(text)
        for m in HAND_SELECTED.finditer(code):
            bad.append(f"{path.name}:{text.count(chr(10), 0, m.start()) + 1}")
    if bad:
        return False, "marks a choice selected by hand; use ForgeUI's forgeChoice style: " + ", ".join(bad)
    return True, ""


# Stars for a rating come from ForgeUI's ForgeRating: one adjustable
# VoiceOver element ("3 of 5 stars"), 44 pt stars, the same colour in every
# app, and `<id>.star.<n>` identifiers. Soundscape and Pocket Library each
# drew their own, with different colours, sizes and VoiceOver (ForgeUI 0.2,
# item 4). This finds a `star` symbol that switches with `star.fill`, the
# shape of a hand-drawn rating.
HAND_STARS = re.compile(r'"star\.fill"\s*:\s*"star"|"star"\s*:\s*"star\.fill"')


def rule_rating(app):
    bad = []
    for path in swift_sources(app):
        text = read(path)
        for m in HAND_STARS.finditer(text):
            bad.append(f"{path.name}:{text.count(chr(10), 0, m.start()) + 1}")
    if bad:
        return False, "draws rating stars by hand; use ForgeUI's ForgeRating: " + ", ".join(bad)
    return True, ""


# A file the app has just written (an export, a backup) goes to the system
# share sheet with ForgeUI's forgeShareFile as soon as it is ready. Four
# apps had their own "your file is ready" sheet or row with a ShareLink on
# it, one tap more and each with its own words and identifiers (app UI
# review, November 2026). A ShareLink for something already there (a link,
# a book's details) is fine. A static check can't see a value's type, so
# this matches the names apps give a written file: `<name>.url` or an
# identifier containing `URL`, unless the name says it is a link (`webURL`,
# `siteURL`, `linkURL`, `pageURL`).
WRITTEN_FILE_SHARE = re.compile(r"\bShareLink\(\s*item:\s*(\w+\.url|\w*URL)\b")
LINK_NAME = re.compile(r"web|site|link|page", re.I)


def rule_share_file(app):
    bad = []
    for path in swift_sources(app):
        text = read(path)
        code = swift_code_mask(text)
        for m in WRITTEN_FILE_SHARE.finditer(code):
            if LINK_NAME.search(m.group(1)):
                continue
            bad.append(f"{path.name}:{text.count(chr(10), 0, m.start()) + 1}")
    if bad:
        return False, "shares a written file from its own sheet or row; use ForgeUI's forgeShareFile: " + ", ".join(bad)
    return True, ""


# A swipe action is hidden: someone who doesn't know to swipe, or uses
# Voice Control or a pointer, never finds it. Rows get their actions with
# ForgeUI's forgeRowActions, which gives the same buttons as swipe actions
# and as a long-press menu. Seven screens in six apps were swipe-only (app
# UI review, November 2026). A file whose rows already have a hand-made
# `.contextMenu` twin passes.
def rule_row_actions(app):
    bad = []
    for path in swift_sources(app):
        text = read(path)
        code = swift_code_mask(text)
        if ".contextMenu" in code:
            continue
        for m in re.finditer(r"\.swipeActions\b", code):
            bad.append(f"{path.name}:{text.count(chr(10), 0, m.start()) + 1}")
    if bad:
        return False, "has swipe-only row actions; use ForgeUI's forgeRowActions (swipe and menu): " + ", ".join(bad)
    return True, ""


# Text is never cut short without a word: Urdu, Naturally's chat kept the
# first 300 characters of a longer line and answered that, with nothing on
# screen to say so (Reuben, 2026-11-22). Typed text over a limit is refused
# with the limit (as Checklist, FieldKit, Good Dog and Pocket Library do), and
# the field shows it. A cut of text nobody typed (an imported name, a summary
# of an article, a model's answer in a prompt) says why on the line or just
# above it with a `// cut:` comment.
RUST_CUT = re.compile(r"\.chars\(\)\s*\.take\(")


def rust_code_mask(text):
    """The Rust code with the insides of comments, strings, raw strings and
    char literals blanked (newlines kept), so `//` in a URL or a call written
    inside a string is not taken for code."""
    out = list(text)
    i, n = 0, len(text)

    def blank(start, end):
        for k in range(start, end):
            if out[k] != "\n":
                out[k] = " "

    while i < n:
        c = text[i]
        if text.startswith("//", i):
            end = text.find("\n", i)
            end = n if end == -1 else end
            blank(i, end)
            i = end
        elif text.startswith("/*", i):
            depth, j = 1, i + 2
            while j < n and depth:
                if text.startswith("/*", j):
                    depth, j = depth + 1, j + 2
                elif text.startswith("*/", j):
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            blank(i, j)
            i = j
        elif (m := re.match(r'b?r(#*)"', text[i:])) and (i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_")):
            close = '"' + m.group(1)
            end = text.find(close, i + m.end())
            end = n if end == -1 else end + len(close)
            blank(i, end)
            i = end
        elif c == '"':
            j = i + 1
            while j < n and text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            blank(i, j + 1)
            i = j + 1
        elif c == "'" and (m := re.match(r"'(?:\\(?:u\{[0-9a-fA-F]+\}|x[0-9a-fA-F]{2}|.)|[^\\'\n])'", text[i:])):
            blank(i, i + m.end())
            i += m.end()
        else:
            i += 1
    return "".join(out)


def without_test_items(code):
    """Blanks each `#[cfg(test)]` item (a module, a function, a `use`) and
    keeps scanning after it."""
    out = list(code)
    for m in re.finditer(r"#\[cfg\(test\)\]", code):
        j = m.end()
        while j < len(code) and code[j] not in "{;":
            j += 1
        if j < len(code) and code[j] == "{":
            depth = 0
            while j < len(code):
                if code[j] == "{":
                    depth += 1
                elif code[j] == "}":
                    depth -= 1
                    if depth == 0:
                        break
                j += 1
        for k in range(m.start(), min(j + 1, len(code))):
            if out[k] != "\n":
                out[k] = " "
    return "".join(out)


def silent_cut_problems(text):
    lines = text.splitlines()
    code = without_test_items(rust_code_mask(text))
    found = []
    for m in RUST_CUT.finditer(code):
        first = code.count("\n", 0, m.start())
        last = code.count("\n", 0, m.end())
        if any("// cut:" in l for l in lines[max(0, first - 3) : last + 1]):
            continue
        found.append(last + 1)
    return found


def rule_no_silent_cut(app):
    if app.name in NO_RUST:
        return None
    bad = []
    for path in rust_sources(app):
        if path.name == "tests.rs" or "bin" in path.parts:
            continue
        for line in silent_cut_problems(read(path)):
            bad.append(f"{path.name}:{line}")
    if bad:
        return False, "cuts text short without saying why; refuse typed text over a limit, or add a `// cut:` reason: " + ", ".join(bad)
    return True, ""


RULES = [
    ("forgekit", "Uses the ForgeKit package", rule_forgekit),
    ("forge-host", "Rust core on the forge-app runtime, hosted by ForgeHost", rule_forge_host),
    ("appkit-alerts", "Errors shown with ForgeAppKit's `forgeAlert`, no own ErrorAlert.swift", rule_appkit_alerts),
    ("errors-shown", "A failed action says so: no `try?` on a change, no empty catch-all", rule_errors_shown),
    ("alerts-on-top", "App-wide alerts on the navigation stack or tabs, never on a first screen that pushes others", rule_alerts_on_top),
    ("alerts-over-sheets", "A model alert is held back while a sheet is up; the sheet shows alerts itself", rule_alerts_over_sheets),
    ("picked-files", "Picked files copied with ForgeAppKit's `ForgePickedFile` (off the main thread), never by hand", rule_picked_files),
    ("web-images", "Pictures from the web through `ForgeRemoteImage` (kept on disk, decoded at the size shown), never `AsyncImage`", rule_web_images),
    ("swipe-guard", "Sheets kept open with ForgeUI's `forgeSwipeGuard` (or `forgeFormToolbar`), never SwiftUI's `interactiveDismissDisabled`", rule_swipe_guard),
    ("forge-theme", "A ForgeUI theme at the root (`.forgeTheme`), so controls wear the app's accent", rule_forge_theme),
    ("state-views", "Empty, no-results, loading and failed screens through ForgeUI's `ForgeStateView`", rule_state_views),
    ("root-loading", "A screen still opening shows `ForgeStateView.loading()`, never a bare `ProgressView()`", rule_root_loading),
    ("toned-text", "Warning, success and error text in ForgeUI's tones, never raw orange, yellow, green or (for errors) red", rule_toned_text),
    ("resume", "Coming back from the background through `ForgeHost.onResume` in the model, never a view watching `scenePhase`", rule_resume),
    ("now-playing", "Lock screen, headphone buttons, calls and unplugged headphones through `ForgeNowPlaying`", rule_now_playing),
    ("floating-bars", "Bars over content (mini-players, banners) through ForgeUI's `forgeBar`", rule_floating_bars),
    ("notices", "Problems in a screen that still works (offline, can't update) through ForgeUI's `ForgeNotice`", rule_notices),
    ("form-toolbar", "Form sheets' Cancel and confirm buttons through ForgeUI's `forgeFormToolbar`, never a hand-built spinner", rule_form_toolbar),
    ("icon-tile", "Coloured icon tiles through `ForgeRow` (a tone or a ForgeUI accent), never a symbol on a raw-coloured rectangle", rule_icon_tile),
    ("choice", "Chips and one-of-several tiles through ForgeUI's `forgeChoice` style, never a hand-added `.isSelected`", rule_choice),
    ("rating", "Ratings through ForgeUI's `ForgeRating`, never stars drawn by hand", rule_rating),
    ("thumbnail", "Pictures in ForgeUI's `ForgeThumbnail`, never an image clipped to a shape by hand", rule_thumbnail),
    ("rename", "Renaming and naming new things through ForgeUI's `forgeRename`, which shows why a name was refused", rule_rename),
    ("share-file", "A file the app wrote (an export, a backup) goes straight to the share sheet with ForgeUI's `forgeShareFile`", rule_share_file),
    ("no-silent-cut", "Typed text over a limit is refused with the limit, never cut short; other cuts say why (`// cut:`)", rule_no_silent_cut),
    ("row-actions", "Row actions through ForgeUI's `forgeRowActions`: a swipe and a long-press menu, never swipe-only", rule_row_actions),
    ("feedback", "Outcome haptics (success, failure) through ForgeAppKit's `ForgeFeedback`", rule_feedback),
    ("forgeui-appearance", "Launch appearance from ForgeUI, no own LaunchAppearance.swift", rule_forgeui_appearance),
    ("urlsession", "iOS requests through the URLSession `http` service", rule_urlsession),
    ("location", "Location through the `location` service", rule_location),
    ("rust-cancellation", "Rust work cancelled from Swift through ForgeKit's `withRustCancellation`", rule_rust_cancellation),
    ("swift-logging", "Swift logs through `ForgeLog`, never `print` or `NSLog`", rule_swift_logging),
    ("connectivity", "Network changes through the `connectivity` service, no own `NWPathMonitor`", rule_connectivity),
    ("requirements", "requirements.toml, every requirement with a test", rule_requirements),
    ("device-checks", "Every requirement has a device scenario", rule_device_checks),
    ("ui-test", "At least one `uitests/*.toml` scenario for `forge ui run`", rule_ui_test),
    ("device-lab", "Reachable from Device Lab's Apps screen", rule_device_lab),
    ("device-family", "Says which devices it runs on (`[ios] devices`)", rule_device_family),
]
RULE_IDS = {r[0] for r in RULES}


# --- the lag list ---------------------------------------------------------

STATUSES = ("missing", "deferred", "not-applicable")

problems = []
lag = {}

# The framework's own hand-written Swift follows the swift-logging rule too:
# the service adapters log to the system log (forgeServicesWarn), since not
# every app exports ForgeLog. UniFFI's generated files are left alone.
FRAMEWORK_SWIFT = ["crates/forge-services/swift", "crates/forge-runtime/swift", "forgekit/Sources"]
for base in FRAMEWORK_SWIFT:
    for path in sorted((root / base).rglob("*.swift")):
        text = read(path)
        if "autogenerated by" in text[:200] or "/Tests/" in str(path):
            continue
        for i, line in enumerate(text.splitlines()):
            if SWIFT_PRINT.search(line.split("//", 1)[0]):
                problems.append(f"{path.relative_to(root)}:{i + 1}: framework Swift logs with print or NSLog; use forgeServicesWarn or ForgeLog")
for entry in tomllib.loads(read(lag_file)).get("lag", []):
    key = (entry.get("app"), entry.get("rule"))
    if key[0] not in apps:
        problems.append(f"docs/app-parity.toml: unknown app {key[0]!r}")
    if key[1] not in RULE_IDS:
        problems.append(f"docs/app-parity.toml: unknown rule {key[1]!r}")
    status = entry.setdefault("status", "missing")
    if status not in STATUSES:
        problems.append(
            f"docs/app-parity.toml: {key[0]}/{key[1]} has status {status!r}; use one of "
            + ", ".join(STATUSES)
        )
    if not entry.get("reason"):
        problems.append(f"docs/app-parity.toml: {key[0]}/{key[1]} needs a reason")
    if status != "not-applicable" and "wave" not in entry:
        problems.append(f"docs/app-parity.toml: {key[0]}/{key[1]} needs the wave that fixes it")
    if status == "not-applicable" and "wave" in entry:
        problems.append(
            f"docs/app-parity.toml: {key[0]}/{key[1]} is not-applicable, so it has no wave"
        )
    if key in lag:
        problems.append(f"docs/app-parity.toml: {key[0]}/{key[1]} is listed twice")
    lag[key] = entry

results = {}
for name in apps:
    app = examples / name
    for rule_id, _, fn in RULES:
        outcome = fn(app)
        results[(name, rule_id)] = outcome
        listed = (name, rule_id) in lag
        if outcome is None:
            if listed:
                problems.append(f"{name}: {rule_id} no longer applies; remove it from docs/app-parity.toml")
            continue
        ok, detail = outcome
        if not ok and not listed:
            problems.append(
                f"{name}: {rule_id}: {detail}. Fix it, or list it in docs/app-parity.toml "
                "with the reason and the wave that fixes it"
            )
        if ok and listed:
            problems.append(f"{name}: {rule_id} passes now; remove it from docs/app-parity.toml")

# The Rust starter is where every app core begins: no silent cuts there either.
for path in sorted((root / "templates" / "app-rust").rglob("*.rs")):
    for line in silent_cut_problems(read(path)):
        problems.append(
            f"{path.relative_to(root)}:{line}: no-silent-cut: text cut short without "
            "a `// cut:` reason"
        )

# The starters are where every app's screens begin, so they meet the screen
# rules too (no lag list for them).
for template in sorted((root / "templates").glob("ios-*")):
    for path in sorted((template / "Sources").rglob("*.swift")):
        text = read(path)
        for line in stack_alert_problems(text):
            problems.append(
                f"{path.relative_to(root)}:{line}: alerts-on-top: app-wide alert on a "
                "screen that pushes others; put it on the NavigationStack"
            )
        for line in dropped_error_problems(text):
            problems.append(
                f"{path.relative_to(root)}:{line}: errors-shown: a failed action is dropped"
            )
        for line in sheet_alert_problems(text):
            problems.append(
                f"{path.relative_to(root)}:{line}: alerts-over-sheets: model alert not held "
                "back while a sheet is up"
            )
        for line in picked_copy_problems(text):
            problems.append(
                f"{path.relative_to(root)}:{line}: picked-files: use ForgePickedFile.copy(of:)"
            )
        for line in raw_tone_problems(text):
            problems.append(
                f"{path.relative_to(root)}:{line}: toned-text: orange or yellow text; "
                "use .forgeForeground(.warning)"
            )


def wave_label(entry):
    return f"wave {entry['wave']}" if entry.get("wave") else "decision"


# --- the generated page ---------------------------------------------------

def cell(name, rule_id):
    outcome = results[(name, rule_id)]
    if outcome is None:
        return "n/a"
    ok, _ = outcome
    if ok:
        return "Yes"
    entry = lag.get((name, rule_id))
    if not entry:
        return "**No**"
    if entry["status"] == "not-applicable":
        return "Not applicable"
    word = "Deferred" if entry["status"] == "deferred" else "No"
    return f"{word} ({wave_label(entry)})"


lines = [
    "# App parity: which apps keep up with the framework",
    "",
    "<!-- Generated by `scripts/check-app-parity.sh --write` from the apps and docs/app-parity.toml. Don't edit by hand. -->",
    "",
    "When the framework gains a better way to do something, every app moves to it, or the lag is written down with the wave of the plan that catches it up. `scripts/check-app-parity.sh` (in `make validate` and Linux CI) fails on a lag that isn't listed in [app-parity.toml](app-parity.toml) and on a listed lag that has been fixed, so the list only shrinks. A framework change that replaces something apps do themselves adds a rule here and moves every app in the same PR, or lists the ones it didn't ([AGENTS.md](../AGENTS.md)). A rule proves adoption: the old way is gone. Where a test can show the framework helper is used correctly (logs actually arrive, a retry repeats the operation), the same PR adds that test too.",
    "",
    "## Rules",
    "",
    "| Rule | What it checks |",
    "|---|---|",
]
lines += [f"| `{rid}` | {desc} |" for rid, desc, _ in RULES]
lines += [
    "",
    "A rule an app doesn't meet is one of: **No** (missing: the app should use the framework and doesn't yet, with the wave that fixes it), **Deferred** (known work postponed on purpose, with the wave) or **Not applicable** (the app has a real reason for its own plumbing). Whether an app has been proven on an iPhone is a separate question, answered in [device-validation.md](device-validation.md).",
]
lines += ["", "## Apps", "", "| App | " + " | ".join(f"`{r[0]}`" for r in RULES) + " |"]
lines.append("|---" * (len(RULES) + 1) + "|")
for name in apps:
    lines.append(f"| {name} | " + " | ".join(cell(name, r[0]) for r in RULES) + " |")
open_lag = {k: e for k, e in lag.items() if e["status"] != "not-applicable"}
lines += ["", "## Known lag", "", "| App | Rule | Status | Why | Wave |", "|---|---|---|---|---|"]
for (name, rule_id), entry in sorted(open_lag.items(), key=lambda kv: (kv[1]["wave"], kv[0])):
    wave = entry["wave"] or "decision"
    lines.append(f"| {name} | `{rule_id}` | {entry['status']} | {entry['reason']} | {wave} |")
not_applicable = {k: e for k, e in lag.items() if e["status"] == "not-applicable"}
if not_applicable:
    lines += ["", "## Not applicable", "", "| App | Rule | Why |", "|---|---|---|"]
    for (name, rule_id), entry in sorted(not_applicable.items()):
        lines.append(f"| {name} | `{rule_id}` | {entry['reason']} |")
generated = "\n".join(lines) + "\n"

if write:
    page.write_text(generated, encoding="utf-8")
    print(f"wrote {page.relative_to(root)}")
elif read(page) != generated:
    problems.append("docs/app-parity.md is out of date: run scripts/check-app-parity.sh --write")

if problems:
    for p in problems:
        print(f"[parity] {p}", file=sys.stderr)
    sys.exit(1)
print(
    f"[parity] {len(apps)} apps, {len(RULES)} rules, {len(open_lag)} known lags, "
    f"{len(not_applicable)} not applicable: OK"
)
PY
