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


# Raw orange or yellow text fails contrast on a light background (the
# friction audit, 2026-11-16); ForgeUI's tones pass in light and dark:
# `.forgeForeground(.warning)`. Fills and tints may stay any colour.
RAW_TONE_TEXT = re.compile(r"\.foregroundStyle\(\s*(?:Color)?\.(orange|yellow)\s*\)")


def raw_tone_problems(text):
    return [
        i + 1
        for i, line in enumerate(text.splitlines())
        if RAW_TONE_TEXT.search(line.split("//", 1)[0])
    ]


def rule_toned_text(app):
    bad = []
    for path in swift_sources(app):
        for line in raw_tone_problems(read(path)):
            bad.append(f"{path.name}:{line}")
    if bad:
        return False, "orange or yellow text (low contrast); use .forgeForeground(.warning): " + ", ".join(bad)
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


RULES = [
    ("forgekit", "Uses the ForgeKit package", rule_forgekit),
    ("forge-host", "Rust core on the forge-app runtime, hosted by ForgeHost", rule_forge_host),
    ("appkit-alerts", "Errors shown with ForgeAppKit's `forgeAlert`, no own ErrorAlert.swift", rule_appkit_alerts),
    ("errors-shown", "A failed action says so: no `try?` on a change, no empty catch-all", rule_errors_shown),
    ("alerts-on-top", "App-wide alerts on the navigation stack or tabs, never on a first screen that pushes others", rule_alerts_on_top),
    ("alerts-over-sheets", "A model alert is held back while a sheet is up; the sheet shows alerts itself", rule_alerts_over_sheets),
    ("picked-files", "Picked files copied with ForgeAppKit's `ForgePickedFile` (off the main thread), never by hand", rule_picked_files),
    ("web-images", "Pictures from the web through `ForgeRemoteImage` (kept on disk, decoded at the size shown), never `AsyncImage`", rule_web_images),
    ("toned-text", "Warning text in ForgeUI's tone, never raw orange or yellow", rule_toned_text),
    ("forgeui-appearance", "Launch appearance from ForgeUI, no own LaunchAppearance.swift", rule_forgeui_appearance),
    ("urlsession", "iOS requests through the URLSession `http` service", rule_urlsession),
    ("location", "Location through the `location` service", rule_location),
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
    "When the framework gains a better way to do something, every app moves to it, or the lag is written down with the wave of the plan that catches it up. `scripts/check-app-parity.sh` (in `make validate` and Linux CI) fails on a lag that isn't listed in [app-parity.toml](app-parity.toml) and on a listed lag that has been fixed, so the list only shrinks. A framework change that replaces something apps do themselves adds a rule here and moves every app in the same PR, or lists the ones it didn't ([AGENTS.md](../AGENTS.md)).",
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
