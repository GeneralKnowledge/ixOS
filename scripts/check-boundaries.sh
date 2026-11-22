#!/usr/bin/env bash
# Checks that dependencies run in the allowed direction between the project's
# areas (docs/areas.md): framework, tools and apps.
#
#   framework  may depend only on framework
#   apps       may depend on framework and on apps
#   tools      may depend on framework and tools
#
# Each Rust crate's area comes from `[package.metadata.forge] area` in its
# Cargo.toml. Crates under crates/ must declare it; crates under examples/
# are apps unless they declare otherwise (Device Lab's crate is tools).
# The root Cargo workspace is for framework and tools: an app crate in it is
# a crossing too. On the Swift side, ForgeUI and ForgeAppKit must stay free
# of the Rust-backed ForgeKit targets.
#
# Known crossings are listed in docs/boundaries.toml with a reason and a
# status: "fix" (to be removed, with the plan that removes it) or "keep" (an
# allowed exception). The check fails on a crossing that isn't listed and on
# a listed one that no longer exists, so the list only shrinks.
#
# Usage: scripts/check-boundaries.sh   (make validate, CI)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
exec python3 -I - "$ROOT" <<'PY'
import pathlib
import re
import sys
import tomllib

root = pathlib.Path(sys.argv[1])
allow_file = root / "docs" / "boundaries.toml"
AREAS = ("framework", "tools", "app")
ALLOWED = {
    "framework": {"framework"},
    "app": {"framework", "app"},
    "tools": {"framework", "tools"},
}
DEP_TABLES = ("dependencies", "dev-dependencies", "build-dependencies")

errors = []


def load(path):
    with path.open("rb") as f:
        return tomllib.load(f)


def manifests():
    for top in ("crates", "examples"):
        for path in sorted((root / top).rglob("Cargo.toml")):
            if "target" in path.relative_to(root).parts:
                continue
            yield path


workspace = load(root / "Cargo.toml")
workspace_members = {
    (root / m).resolve() for m in workspace["workspace"].get("members", [])
}
workspace_deps = workspace["workspace"].get("dependencies", {})

# name -> (dir, area)
crates = {}
by_dir = {}
for path in manifests():
    data = load(path)
    pkg = data.get("package")
    if not pkg:
        continue
    name = pkg["name"]
    rel = path.parent.relative_to(root)
    area = pkg.get("metadata", {}).get("forge", {}).get("area")
    if area is None:
        if rel.parts[0] == "crates":
            errors.append(
                f"{rel}/Cargo.toml: no [package.metadata.forge] area "
                f"(one of {', '.join(AREAS)}; docs/areas.md)"
            )
            continue
        area = "app"
    if area not in AREAS:
        errors.append(f"{rel}/Cargo.toml: unknown area {area!r}")
        continue
    crates[name] = (path, area)
    by_dir[path.parent.resolve()] = name


def dep_tables(data):
    for table in DEP_TABLES:
        yield data.get(table, {})
    for target in data.get("target", {}).values():
        for table in DEP_TABLES:
            yield target.get(table, {})


def local_dep(manifest, key, spec):
    """The in-repo crate a dependency entry points at, or None."""
    if not isinstance(spec, dict):
        return None
    if spec.get("workspace"):
        spec = workspace_deps.get(key, {})
        base = root
    else:
        base = manifest.parent
    if not isinstance(spec, dict) or "path" not in spec:
        return None
    return by_dir.get((base / spec["path"]).resolve())


found = set()
for name, (path, area) in crates.items():
    data = load(path)
    for table in dep_tables(data):
        for key, spec in table.items():
            dep = local_dep(path, key, spec)
            if dep is None or dep == name:
                continue
            dep_area = crates[dep][1]
            if dep_area not in ALLOWED[area]:
                found.add(("depends", name, dep))
    if area == "app" and path.parent.resolve() in workspace_members:
        found.add(("workspace", name, "root"))

# Swift: the design system links no Rust.
package = (root / "forgekit" / "Package.swift").read_text(encoding="utf-8")
rust_backed = {"ForgeApp", "ForgeKit", "ForgeNative", "forge_appFFI", "forge_nativeFFI"}
for target in ("ForgeUI", "ForgeAppKit"):
    m = re.search(
        r'\.target\(\s*name:\s*"' + target + r'"(.*?)\n\s*\)', package, re.S
    )
    if not m:
        errors.append(f"forgekit/Package.swift: target {target} not found")
        continue
    deps = set(re.findall(r'"([A-Za-z_]+)"', m.group(1).split("path:")[0]))
    for dep in sorted(deps & rust_backed):
        found.add(("depends", target, dep))

listed = {}
for entry in load(allow_file).get("crossing", []):
    if not entry.get("from") or not entry.get("to"):
        errors.append(f"docs/boundaries.toml: an entry without from and to: {entry}")
        continue
    key = (entry.get("kind", "depends"), entry["from"], entry["to"])
    status = entry.get("status", "fix")
    if status not in ("fix", "keep"):
        errors.append(f"docs/boundaries.toml: {key}: unknown status {status!r}")
    if not entry.get("reason"):
        errors.append(f"docs/boundaries.toml: {key}: no reason")
    if status == "fix" and not entry.get("plan"):
        errors.append(f"docs/boundaries.toml: {key}: a 'fix' entry names its plan")
    listed[key] = entry


def describe(key):
    kind, src, dst = key
    if kind == "workspace":
        return f"app crate {src} is a member of the root Cargo workspace"
    a = crates.get(src, (None, "framework (Swift)"))[1]
    b = crates.get(dst, (None, "Rust-backed ForgeKit target"))[1]
    return f"{src} ({a}) depends on {dst} ({b})"


def covered(key):
    """The listed entry for a crossing: exact, or `to = "*"` for every app
    crate the `from` crate depends on (a host such as Device Lab)."""
    if key in listed:
        return key
    wild = (key[0], key[1], "*")
    if wild in listed and crates.get(key[2], (None, ""))[1] == "app":
        return wild
    return None


used = {covered(k) for k in found} - {None}
for key in sorted(k for k in found if covered(k) is None):
    errors.append(
        f"crossing not allowed: {describe(key)}. Remove it, or list it in "
        f"docs/boundaries.toml with the reason (docs/areas.md)"
    )
for key in sorted(listed.keys() - used):
    errors.append(
        f"docs/boundaries.toml lists a crossing that no longer exists: "
        f"{key[1]} -> {key[2]}. Delete the entry"
    )

# Third-party Swift packages. ixOS has one, approved for a test only:
# Google's A2UI renderer in Device Lab (A2UI-1, Reuben 2026-11-22,
# docs/research/external-projects/assessments/ai/a2ui.md). Anything else,
# or A2UI anywhere but Device Lab (ForgeKit, an app, the templates), fails.
SWIFT_PACKAGES_ALLOWED = {
    ("examples/device-lab/Package.swift", "https://github.com/a2ui-project/a2ui"),
}
for manifest in sorted(root.rglob("Package.swift")):
    rel = manifest.relative_to(root).as_posix()
    if any(part in (".build", "target", "node_modules") for part in manifest.parts):
        continue
    # Drop comments, then scan the whole manifest, so a declaration split
    # over several lines is caught the same as one on a single line.
    text = re.sub(r"/\*.*?\*/", "", manifest.read_text(), flags=re.S)
    text = "\n".join(
        l for l in text.splitlines() if not l.strip().startswith("//")
    )
    # Every `.package(` must be one of two plain shapes, read whole up to its
    # closing bracket (brackets in strings and comments don't count):
    #   .package(path: "<literal>")                   a local package, or
    #   .package(url: "<literal>", <requirement>)     with an allowed URL.
    # (`name:` may come first in either.) Anything else, such as a URL
    # built from pieces or by a helper, fails: the check can't read it.
    local_re = re.compile(r'\s*(?:name:\s*"[^"]*"\s*,\s*)?path:\s*"[^"]*"\s*')
    remote_re = re.compile(
        r'\s*(?:name:\s*"[^"]*"\s*,\s*)?url:\s*(#*)"([^"]*)"\1\s*(?:,|$)'
    )
    for m in re.finditer(r"\.package\s*\(", text):
        depth, quoted, end, decl = 1, False, m.end(), []
        while end < len(text):
            c = text[end]
            if not quoted and text.startswith("//", end):
                end = text.find("\n", end)
                end = len(text) if end < 0 else end
                continue
            if c == '"':
                quoted = not quoted
            elif not quoted:
                depth += {"(": 1, ")": -1}.get(c, 0)
                if depth == 0:
                    break
            decl.append(c)
            end += 1
        decl = "".join(decl)
        if local_re.fullmatch(decl):
            continue
        remote = remote_re.match(decl)
        if not remote:
            errors.append(
                f"{rel}: a .package declaration the check can't read; write "
                f'.package(path: "...") or .package(url: "...", ...) with a '
                f"plain URL (scripts/check-boundaries.sh)"
            )
            continue
        url = remote.group(2)
        if (rel, url.removesuffix(".git")) not in SWIFT_PACKAGES_ALLOWED:
            errors.append(
                f"{rel}: third-party Swift package {url} isn't allowed here "
                f"(scripts/check-boundaries.sh, SWIFT_PACKAGES_ALLOWED)"
            )

if errors:
    for e in errors:
        print(f"error: {e}", file=sys.stderr)
    sys.exit(1)

keep = sum(1 for e in listed.values() if e.get("status", "fix") == "keep")
print(
    f"boundaries ok: {len(crates)} crates; {len(listed) - keep} crossing(s) "
    f"to fix and {keep} allowed exception(s), all listed in docs/boundaries.toml"
)
PY
