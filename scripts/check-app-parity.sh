#!/usr/bin/env bash
# Checks that every example app keeps up with the framework (docs/app-parity.md).
#
# Each rule is a plain fact about an app's files: it hosts its Rust core with
# ForgeHost instead of its own lifecycle code, sends iOS requests through the
# URLSession `http` service, has a smoke test, is reachable from Device Lab,
# and so on. Apps that lag today are listed in docs/app-parity.toml with the
# reason and the plan wave that catches them up. The check fails on a lag
# that isn't listed (a new copy can't slip in) and on a listed lag that has
# been fixed (so the list only shrinks). It also checks that
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


RULES = [
    ("forgekit", "Uses the ForgeKit package", rule_forgekit),
    ("forge-host", "Rust core on the forge-app runtime, hosted by ForgeHost", rule_forge_host),
    ("appkit-alerts", "Errors shown with ForgeAppKit's `forgeAlert`, no own ErrorAlert.swift", rule_appkit_alerts),
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

problems = []
lag = {}
for entry in tomllib.loads(read(lag_file)).get("lag", []):
    key = (entry.get("app"), entry.get("rule"))
    if key[0] not in apps:
        problems.append(f"docs/app-parity.toml: unknown app {key[0]!r}")
    if key[1] not in RULE_IDS:
        problems.append(f"docs/app-parity.toml: unknown rule {key[1]!r}")
    if not entry.get("reason") or "wave" not in entry:
        problems.append(f"docs/app-parity.toml: {key[0]}/{key[1]} needs a reason and a wave")
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
    return f"No (wave {entry['wave']})" if entry["wave"] else "No (decision)"


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
lines += ["", "## Apps", "", "| App | " + " | ".join(f"`{r[0]}`" for r in RULES) + " |"]
lines.append("|---" * (len(RULES) + 1) + "|")
for name in apps:
    lines.append(f"| {name} | " + " | ".join(cell(name, r[0]) for r in RULES) + " |")
lines += ["", "## Known lag", "", "| App | Rule | Why | Wave |", "|---|---|---|---|"]
for (name, rule_id), entry in sorted(lag.items(), key=lambda kv: (kv[1]["wave"], kv[0])):
    wave = entry["wave"] or "decision"
    lines.append(f"| {name} | `{rule_id}` | {entry['reason']} | {wave} |")
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
print(f"[parity] {len(apps)} apps, {len(RULES)} rules, {len(lag)} known lags: OK")
PY
