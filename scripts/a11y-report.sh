#!/usr/bin/env bash
# Accessibility report for every app's Swift sources, the starter templates
# and ForgeKit, made by a11y-check (CVS Health, Apache-2.0), a static checker
# built on SwiftSyntax. REPORT ONLY: it never fails because of a finding.
# See docs/accessibility-report.md and the assessment in
# docs/research/external-projects/assessments/accessibility/a11y-check.md.
#
#   scripts/a11y-report.sh [OUT_DIR]
#
# Builds the pinned a11y-check once (about 7 minutes from cold) into
# $A11Y_CHECK_HOME (default ~/.cache/forge/a11y-check/<tag>) and reuses it.
# Needs Swift 6 on PATH, or FORGE_SWIFT_DOCKER=<image> (CI uses swift:6.2) to
# build inside that image. Writes OUT_DIR/<target>.json (default
# target/a11y-report) and prints a summary table, also appended to
# $GITHUB_STEP_SUMMARY when it is set.
#
# Exit status: 0 whatever the findings; non-zero only when the tool can't be
# fetched, built or run.
set -euo pipefail

TAG="0.7.0"
COMMIT="cccad9dbfc39f086cfb8ae3acd0af52883c9b5c7"
REPO="https://github.com/cvs-health/ios-swiftui-accessibility-techniques"

root="$(cd "$(dirname "$0")/.." && pwd)"
out="${1:-$root/target/a11y-report}"
home="${A11Y_CHECK_HOME:-${XDG_CACHE_HOME:-$HOME/.cache}/forge/a11y-check/$TAG}"
bin="$home/a11y-check"

build() {
    local src="$home/src"
    rm -rf "$src"
    mkdir -p "$src"
    git -C "$src" init -q
    git -C "$src" fetch -q --depth 1 "$REPO" "$COMMIT"
    git -C "$src" checkout -q FETCH_HEAD
    local cmd="swift build -c release --static-swift-stdlib --product A11yCheckCLI"
    if [ -n "${FORGE_SWIFT_DOCKER:-}" ]; then
        docker run --rm --network host \
            -e HTTPS_PROXY -e https_proxy -e SSL_CERT_FILE -e GIT_SSL_CAINFO \
            ${SSL_CERT_FILE:+-v "$SSL_CERT_FILE:$SSL_CERT_FILE:ro"} \
            -v "$src:/src" -w /src "$FORGE_SWIFT_DOCKER" \
            bash -c "$cmd && chown -R $(id -u):$(id -g) .build"
    else
        command -v swift >/dev/null || {
            echo "a11y-report: Swift 6 isn't on PATH; install it or set FORGE_SWIFT_DOCKER=swift:6.2" >&2
            exit 2
        }
        (cd "$src" && $cmd)
    fi
    cp "$src/.build/release/A11yCheckCLI" "$bin"
    rm -rf "$src"
}

if [ ! -x "$bin" ]; then
    echo "a11y-report: building a11y-check $TAG (${COMMIT:0:7}) into $home"
    build
fi

mkdir -p "$out"
targets=()
for app in "$root"/examples/*/; do
    [ -d "$app/Sources" ] && targets+=("$(basename "$app"):${app}Sources")
done
targets+=("forgekit:$root/forgekit/Sources" "templates:$root/templates")

summary="$out/summary.md"
{
    echo "### Accessibility report (a11y-check $TAG, report only)"
    echo
    echo "Findings are not failures: see docs/accessibility-report.md for which rules are known to be noisy."
    echo
    echo "| Sources | Errors | Warnings | Most frequent rules |"
    echo "|---|---|---|---|"
} >"$summary"

for entry in "${targets[@]}"; do
    name="${entry%%:*}"
    path="${entry#*:}"
    # a11y-check exits non-zero when it reports errors; that is a finding,
    # not a failure of this script. A crash leaves no JSON and is caught below.
    "$bin" "$path" --config "$root/.a11ycheck.yml" --format json --no-trend \
        --base-path "$root/" >"$out/$name.json" 2>"$out/$name.err" || true
    python3 - "$out/$name.json" "$name" >>"$summary" <<'PY'
import collections, json, sys
path, name = sys.argv[1], sys.argv[2]
try:
    data = json.load(open(path))
except (OSError, ValueError):
    sys.exit(f"a11y-report: a11y-check produced no report for {name}")
diags = data.get("diagnostics", [])
sev = collections.Counter(d["severity"] for d in diags)
top = collections.Counter(d["ruleID"] for d in diags).most_common(3)
rules = ", ".join(f"{r} ({n})" for r, n in top) or "none"
print(f"| {name} | {sev.get('error', 0)} | {sev.get('warning', 0)} | {rules} |")
PY
done

cat "$summary"
if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    cat "$summary" >>"$GITHUB_STEP_SUMMARY"
fi
echo "a11y-report: JSON reports in $out"
