#!/usr/bin/env bash
# The human-only workflow (docs/human-workflow.md), automated as far as a
# Linux machine without an iPhone allows: a person creates an app with the
# default generator, outside the Forge checkout, and develops it with the
# forge CLI, cargo and a text editor. No AI agent, no MCP server, no API key,
# no agent configuration and no hosted service.
#
# Each step is one numbered step of the doc. Steps that need the iOS SDK or
# an iPhone run only when asked (FORGE_HUMAN_DEVICE=1, with xtool and a
# phone); otherwise they are listed as manual at the end, never reported as
# passing.
#
# Usage: scripts/check-human-workflow.sh [work-dir]   (default: a new temp dir)
#   FORGE_SWIFT_DOCKER=swift:6.2  run `forge check swift` in that image when
#                                 swift isn't on PATH (as CI does)
#   FORGE_HUMAN_DEVICE=1          also build, install, launch, read logs and
#                                 run the UI test on the connected iPhone
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="${1:-$(mktemp -d)}"
mkdir -p "$WORK"
WORK="$(cd "$WORK" && pwd)"
case "$WORK/" in
  "$ROOT"/*) echo "error: work dir must be outside the checkout ($ROOT)" >&2; exit 1 ;;
esac

step() { echo; echo "[human-workflow] $*"; }
fail() { echo "[human-workflow] FAIL: $*" >&2; exit 1; }
MANUAL=()
manual() { MANUAL+=("$*"); }

# Nothing agent-shaped may be in play: not inherited from the caller, not
# started by any step.
unset FORGE_MCP_CHILD ANTHROPIC_API_KEY OPENAI_API_KEY || true

step "1. Install: build the forge CLI from the checkout"
cargo build -q --manifest-path "$ROOT/Cargo.toml" -p forge-cli
FORGE="$ROOT/target/debug/forge"
export FORGE_WORKSPACE="$ROOT"
"$FORGE" --version
"$FORGE" doctor || true   # readiness report; a Linux box without xtool is fine here

START="$(date -u +%Y-%m-%dT%H-%M-%S)"
APP="$WORK/Shelf"
cd "$WORK"

step "2. Create an app with the default generator"
"$FORGE" init Shelf --bundle-id org.example.shelf --path "$APP"
for f in AGENTS.md CLAUDE.md .mcp.json; do
  [[ ! -e "$APP/$f" ]] || fail "the default app has $f: agent files must be opt-in (forge agent setup)"
done
# rustup reads the pin from the current folder up, so cargo works in the
# app folder even with no rustup default (Reuben's laptop, 2026-11-10).
[[ -f "$APP/rust-toolchain.toml" ]] || fail "the app has no rust-toolchain.toml at its top: cargo needs a rustup default there"
cd "$APP"
git init -q && git add -A && git -c user.name=person -c user.email=person@example.com commit -qm "New app"

step "3. Inspect the generated project"
"$FORGE" inspect
"$FORGE" --json inspect >"$WORK/inspect.json"
grep -q '"present":\[\]' "$WORK/inspect.json" || fail "forge inspect lists agent files in a default app"
grep -q '"id":"notes.add"' "$WORK/inspect.json" || fail "forge inspect does not list the starter's identifiers"
"$FORGE" config show >/dev/null

step "4. Change the Rust business logic (a new rule, with its test)"
cat >>rust/src/lib.rs <<'RUST'

/// Minutes to read `text` at 200 words a minute, rounded up; at least 1.
#[uniffi::export]
pub fn reading_minutes(text: String) -> u32 {
    let words = text.split_whitespace().count() as u32;
    words.div_ceil(200).max(1)
}

#[cfg(test)]
mod reading_time_tests {
    use super::reading_minutes;

    #[test]
    fn rounds_up_and_is_never_zero() {
        assert_eq!(reading_minutes(String::new()), 1);
        assert_eq!(reading_minutes("word ".repeat(200)), 1);
        assert_eq!(reading_minutes("word ".repeat(201)), 2);
    }
}
RUST
cargo test -q --manifest-path rust/Cargo.toml reading_time
"$FORGE" bindings generate
grep -q 'func readingMinutes(text: String) ' Generated/ForgeFFI/forge_ffi.swift \
  || fail "the Swift bindings do not have readingMinutes"

step "5. Change the SwiftUI presentation (and the model test that covers it)"
# The row shows the reading time Rust works out; Swift adds no rule.
python3 - "$APP/Sources/AppHost/NoteListView.swift" <<'PY'
import sys
p = sys.argv[1]
s = open(p).read()
old = 'Text(note.preview ?? "No additional text")'
assert old in s, "NoteListView.swift changed shape"
s = s.replace(old, 'Text("\\(readingMinutes(text: note.text)) min read")\n                ' + old, 1)
open(p, "w").write(s)
PY
grep -q 'min read' Sources/AppHost/NoteListView.swift || fail "the view edit did not apply"
cat >Tests/AppHostTests/ReadingTimeTests.swift <<'SWIFT'
@preconcurrency import ForgeFFI
import XCTest

final class ReadingTimeTests: XCTestCase {
    func testReadingTimeRoundsUp() {
        XCTAssertEqual(readingMinutes(text: ""), 1)
        XCTAssertEqual(readingMinutes(text: String(repeating: "word ", count: 401)), 3)
    }
}
SWIFT
cat >requirements.toml <<'TOML'
[[requirement]]
id = "RT-1"
statement = "Each note in the list says how long it takes to read, rounded up to whole minutes."
twin = ["ReadingTimeTests.testReadingTimeRoundsUp"]
TOML

step "6. Run the tests and checks"
"$FORGE" test
SWIFT_RAN=0
if [[ -n "${FORGE_SWIFT_DOCKER:-}" ]] || command -v swift >/dev/null 2>&1; then
  "$FORGE" check swift
  SWIFT_RAN=1
else
  echo "no swift on PATH and FORGE_SWIFT_DOCKER unset: forge check swift skipped"
  manual "6. forge check swift (needs the Swift toolchain or Docker): the model, bindings and ReadingTimeTests on Linux"
fi
"$FORGE" ui focus --changed Sources/AppHost/NoteListView.swift
git add -A && git -c user.name=person -c user.email=person@example.com commit -qm "Show reading time"

if [[ "${FORGE_HUMAN_DEVICE:-}" == 1 ]]; then
  step "7-11. Build, install, launch, inspect, logs and a UI test on the iPhone"
  "$FORGE" build ios
  "$FORGE" run ios
  "$FORGE" ui setup
  "$FORGE" ui launch
  "$FORGE" ui screenshot "$WORK/launched.png"
  "$FORGE" ui audit || true
  # One action at a time from the terminal, no test file: open the compose
  # sheet, type, hide the keyboard, check, cancel.
  "$FORGE" ui tap --id notes.add
  "$FORGE" ui type --id compose.text "Hello from the terminal"
  "$FORGE" ui key dismiss
  "$FORGE" ui expect --id compose.save --enabled true
  "$FORGE" ui tap --id compose.cancel
  "$FORGE" device logs --process ShelfXtool --for 10s
  "$FORGE" ui run uitests/smoke.toml --visual record
  "$FORGE" diagnose --for 0
else
  manual "7. forge build ios (needs xtool and the Darwin SDK): the SwiftUI change compiles"
  manual "8. forge run ios (needs an iPhone): signed and installed"
  manual "9. forge ui setup, then forge ui launch, forge ui screenshot, forge ui audit: the row shows \"N min read\"; forge ui tap/type/key/expect act one step at a time"
  manual "10. forge device logs --process ShelfXtool --for 20s, forge device crashes, forge diagnose"
  manual "11. forge ui run uitests/smoke.toml: passes on the phone and writes a device evidence bundle"
fi

step "9b. One-step UI commands exist for a person (no test file, no MCP)"
for verb in tap type clear scroll swipe long-press double-tap drag key expect; do
  "$FORGE" ui "$verb" --help >/dev/null || fail "forge ui $verb is missing"
done
# A wrong target is refused before any phone is needed.
if "$FORGE" ui drag --id notes.add >/dev/null 2>&1; then fail "forge ui drag without --to-id or --by was accepted"; fi

step "12. Review the evidence"
if [[ $SWIFT_RAN -eq 1 ]]; then
  "$FORGE" verify --twin-only
  "$FORGE" --json verify --twin-only >"$WORK/verify.json" || true
  grep -q '"status":"proven"' "$WORK/verify.json" || fail "RT-1 is not proven by its twin test: $(cat "$WORK/verify.json")"
else
  "$FORGE" verify --twin-only || true   # no twin evidence without swift: says so
fi
"$FORGE" verify --run "$START"
report="$(ls -d .forge/runs/*-run | tail -1)/report.md"
[[ -f "$report" ]] || fail "forge verify --run wrote no report.md"
grep -q 'Commands and tool calls recorded' "$report" || fail "the run record does not count the terminal's commands: $(cat "$report")"
[[ -s .forge/history.jsonl ]] || fail "no command history in .forge/history.jsonl"
manual "12. forge verify --accept RT-1 --note \"…\" at your own terminal, after looking at the phone: only a person records an acceptance"

step "No agent was involved"
for f in AGENTS.md CLAUDE.md .mcp.json; do
  [[ ! -e "$APP/$f" ]] || fail "$f appeared during the workflow"
done
if ls -d .forge/runs/*-mcp >/dev/null 2>&1; then fail "an MCP journal exists: forge mcp ran"; fi
if grep -q '"via":"mcp"' .forge/history.jsonl; then fail "the history records an MCP call"; fi

step "OK: a fresh app in $APP was created, changed, tested and reviewed without an agent"
if ((${#MANUAL[@]})); then
  echo "Not run here (manual or device steps, see docs/human-workflow.md):"
  printf '  - %s\n' "${MANUAL[@]}"
fi
