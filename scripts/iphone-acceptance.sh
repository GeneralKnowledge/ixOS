#!/usr/bin/env bash
# One repeatable iPhone acceptance run for the Linux dev loop: signing, the
# USB connection, install, launch and relaunch, UI inspection (WebDriverAgent
# and, optionally, Apple's accessibility service), the Pocket Library UI test,
# logs and crash reports. Every step runs a documented `forge --json` command
# and is recorded as PASS, FAIL or SKIP with its raw result.
#
#   scripts/iphone-acceptance.sh [--app examples/pocket-library] [--out DIR] [--with-ax-probe] [--with-mcp]
#
# Before: the iPhone is plugged in, unlocked and trusts this computer;
# `forge ui setup` has been run since the phone last restarted (or set
# FORGE_DDI and FORGE_WDA_RUNNER and the script runs it). Keep the phone
# unlocked until the end. Nothing is removed from the phone; the only install
# is the app under test (`forge run ios`), over its existing copy.
#
# Results: <out>/results.md (the table to paste back) and <out>/results.json,
# plus every step's raw output in <out>/steps/. A SKIP is never a pass.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FORGE="${FORGE:-forge}"
APP="examples/pocket-library"
OUT=""
WITH_AX=0
WITH_MCP=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --app) APP="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --with-ax-probe) WITH_AX=1; shift ;;
    --with-mcp) WITH_MCP=1; shift ;;
    -h|--help) sed -n '2,17p' "$0"; exit 0 ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done
[[ "$APP" = /* ]] || APP="$ROOT/$APP"
OUT="${OUT:-$PWD/iphone-acceptance-$(date -u +%Y%m%dT%H%M%SZ)}"
mkdir -p "$OUT/steps"
RESULTS="$OUT/results.tsv"
: > "$RESULTS"

command -v python3 >/dev/null || { echo "python3 is needed to read the JSON results" >&2; exit 2; }

# json FILE EXPR: a Python expression on the parsed result `r` (empty on error).
json() {
  python3 - "$1" "$2" <<'PY' 2>/dev/null
import json, sys
try:
    r = json.load(open(sys.argv[1]))
    v = eval(sys.argv[2], {"r": r})
    print("" if v is None else (json.dumps(v) if isinstance(v, (dict, list)) else v))
except Exception:
    pass
PY
}

# record ID KIND STATUS DETAIL
record() {
  printf '%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "${4//$'\t'/ }" >> "$RESULTS"
  printf '%-4s %-9s %-4s %s\n' "$1" "$2" "$3" "$4"
}

# step ID KIND NAME -- forge args…: runs `forge --json …`, records ok/stage/error.
step() {
  local id="$1" kind="$2" name="$3"; shift 4
  local file="$OUT/steps/$id.json"
  "$FORGE" --json "$@" > "$file" 2> "$OUT/steps/$id.stderr"
  local ok stage err
  ok="$(json "$file" 'r.get("ok")')"
  stage="$(json "$file" 'r.get("stage")')"
  err="$(json "$file" '(r["hints"][0]["problem"] if r.get("hints") else (r.get("error") or "").splitlines()[0]) if not r.get("ok") else ""')"
  if [[ "$ok" == "True" ]]; then
    record "$id" "$kind" PASS "$name"
    return 0
  fi
  record "$id" "$kind" FAIL "$name: stage ${stage:-none}${err:+: $err} (steps/$id.json)"
  return 1
}

skip() { record "$1" "$2" SKIP "$3"; }

has_ui=0
"$FORGE" ui --help >/dev/null 2>&1 && has_ui=1

echo "iPhone acceptance run → $OUT"
echo "App under test: $APP"
echo

# --- S. Signing --------------------------------------------------------------
"$FORGE" doctor --json > "$OUT/steps/S1.json" 2> "$OUT/steps/S1.stderr"
team="$(json "$OUT/steps/S1.json" 'r["ios_device"]["signing_team"]')"
kind="$(json "$OUT/steps/S1.json" 'r["ios_device"]["signing_team"]["kind"]')"
signin="$(json "$OUT/steps/S1.json" 'r["ios_device"]["apple_sign_in"]')"
if [[ "$signin" == "logged_in" && -n "$kind" && "$kind" != "unknown" ]]; then
  record S1 mandatory PASS "xtool signed in; team: $(json "$OUT/steps/S1.json" 'r["ios_device"]["signing_team"]["name"] or r["ios_device"]["signing_team"]["id"]') ($kind)"
else
  record S1 mandatory FAIL "sign-in ${signin:-unknown}, team ${team:-unknown} (forge doctor)"
fi
if [[ "$kind" == "paid" ]]; then
  record S2 mandatory PASS "the selected team is the paid Apple Developer Program team"
else
  record S2 mandatory FAIL "the selected team is ${kind:-unknown}, not paid: xtool auth logout && xtool auth login, pick the paid team (docs/ios-signing.md)"
fi

# --- D. Device connection ------------------------------------------------------
step D1 mandatory "iPhone detected over USB (forge device list)" -- device list
if step D2 mandatory "device details (forge device info)" -- device info; then
  dm="$(json "$OUT/steps/D2.json" 'r["data"].get("developer_mode") if isinstance(r["data"], dict) else r["data"][0].get("developer_mode")')"
  [[ -n "$dm" ]] && record D3 mandatory "$([[ "$dm" == "True" ]] && echo PASS || echo FAIL)" "Developer Mode: $dm"
fi

# --- I. Build, sign, install ---------------------------------------------------
step I0 mandatory "app configuration (forge config show)" -- config show --project "$APP"
product="$(json "$OUT/steps/I0.json" 'r["data"]["xtool_product"]')"
bundle="$(json "$OUT/steps/I0.json" 'r["data"]["bundle_id"]')"
marker="$OUT/steps/.before-install"
touch "$marker"
if step I1 mandatory "build, sign and install (forge run ios)" -- run ios --project "$APP"; then
  # The signing identity and entitlements xtool used: the provisioning
  # profile it embedded in the app it just built.
  profile="$(find "$APP" -name embedded.mobileprovision -newer "$marker" 2>/dev/null | head -1)"
  if [[ -n "$profile" ]]; then
    python3 - "$profile" "$OUT/steps/I2.json" <<'PY'
import plistlib, sys, json, re
raw = open(sys.argv[1], "rb").read()
start, end = raw.find(b"<?xml"), raw.find(b"</plist>")
p = plistlib.loads(raw[start:end + len(b"</plist>")])
days = (p["ExpirationDate"] - p["CreationDate"]).days
ent = p.get("Entitlements", {})
json.dump({
    "team_name": p.get("TeamName"), "team_id": (p.get("TeamIdentifier") or [None])[0],
    "app_id_name": p.get("AppIDName"), "created": str(p["CreationDate"]),
    "expires": str(p["ExpirationDate"]), "days_valid": days,
    "devices": len(p.get("ProvisionedDevices", [])),
    "entitlements": sorted(ent.keys()), "get_task_allow": ent.get("get-task-allow"),
    "application_identifier": ent.get("application-identifier"),
}, open(sys.argv[2], "w"), indent=2)
PY
    days="$(json "$OUT/steps/I2.json" 'r["days_valid"]')"
    record I2 mandatory PASS "profile: team $(json "$OUT/steps/I2.json" 'r["team_name"]') ($(json "$OUT/steps/I2.json" 'r["team_id"]')), valid $days days, entitlements $(json "$OUT/steps/I2.json" '", ".join(r["entitlements"])') (steps/I2.json)"
    if [[ "$kind" == "paid" && -n "$days" && "$days" -le 7 ]]; then
      record I3 mandatory FAIL "doctor says paid but the profile lasts $days days: a free-team profile"
    elif [[ -n "$days" ]]; then
      record I3 mandatory PASS "profile lifetime ($days days) matches the $kind team"
    fi
  else
    skip I2 mandatory "no embedded.mobileprovision newer than this run under $APP: check where xtool put the .app"
  fi
fi

# --- L. Launch, relaunch, background, UI -----------------------------------------
if [[ $has_ui -eq 0 ]]; then
  for id in L1 U1 U2 U3 U4; do skip "$id" mandatory "this forge has no \`forge ui\` (PR #61 not merged/installed)"; done
else
  if [[ -n "${FORGE_DDI:-}" || -n "${FORGE_WDA_RUNNER:-}" ]]; then
    step L0 mandatory "UI runner setup (forge ui setup)" -- ui setup ${FORGE_DDI:+--ddi "$FORGE_DDI"} ${FORGE_WDA_RUNNER:+--runner "$FORGE_WDA_RUNNER"}
  fi
  # Logs are live only: capture while the lifecycle test and UI test run.
  "$FORGE" --json device logs --process "${product:-AppHostXtool}" --for 90s --limit 2000 \
    > "$OUT/steps/R1.json" 2> "$OUT/steps/R1.stderr" &
  logs_pid=$!
  cat > "$OUT/steps/lifecycle.toml" <<EOF
name = "Acceptance: launch, background, terminate, relaunch"
app = "$bundle"

[[step]]
launch = {}

[[step]]
expect = { id = "library.add" }

[[step]]
background = 3

[[step]]
expect = { id = "library.add" }

[[step]]
terminate = {}

[[step]]
expect = { id = "library.add", gone = true }

[[step]]
launch = {}

[[step]]
expect = { id = "library.add" }

[[step]]
screenshot = "relaunched"
EOF
  step L1 mandatory "launch, background, terminate, relaunch, still running (forge ui run lifecycle.toml)" -- ui run "$OUT/steps/lifecycle.toml" --out "$OUT/lifecycle"
  step U1 mandatory "Pocket Library UI test: tap, type, assert, screenshot (forge ui run m0.toml)" -- ui run "$APP/uitests/m0.toml" --out "$OUT/ui-m0"
  step U2 mandatory "accessibility tree through WebDriverAgent (forge ui tree)" -- ui tree --app "$bundle"
  cp "$OUT/steps/U2.json" "$OUT/wda-tree.json" 2>/dev/null
  step U3 mandatory "screenshot (forge ui screenshot)" -- ui screenshot "$OUT/screenshot.png"
  # A step that must fail: the diagnostics have to say which step and why.
  cat > "$OUT/steps/must-fail.toml" <<EOF
name = "Acceptance: a missing element is reported, not passed"
app = "$bundle"
timeout = 3

[[step]]
tap = { id = "acceptance.does-not-exist" }
EOF
  "$FORGE" --json ui run "$OUT/steps/must-fail.toml" --out "$OUT/must-fail" > "$OUT/steps/U4.json" 2> "$OUT/steps/U4.stderr"
  if [[ "$(json "$OUT/steps/U4.json" 'r.get("ok")')" == "False" && -n "$(json "$OUT/steps/U4.json" 'r["data"]["failed_step"]')" ]]; then
    record U4 mandatory PASS "a missing element fails with failed_step, tree and screenshot (steps/U4.json, must-fail/)"
  else
    record U4 mandatory FAIL "a missing element did not fail with failed_step (steps/U4.json)"
  fi
  wait "$logs_pid"
  lines="$(json "$OUT/steps/R1.json" 'len(r["data"]["lines"])')"
  if [[ "$(json "$OUT/steps/R1.json" 'r.get("ok")')" == "True" && "${lines:-0}" -gt 0 ]]; then
    record R1 mandatory PASS "$lines log lines from ${product:-AppHostXtool} while the tests ran"
  else
    record R1 mandatory FAIL "no log lines from ${product:-AppHostXtool} (steps/R1.json)"
  fi
fi

# --- R. Runtime diagnostics ------------------------------------------------------
step R2 mandatory "crash reports listed and copied (forge device crashes)" -- device crashes --app "${product:-AppHostXtool}" --pull "$OUT/crashes"

# --- Optional experiments ----------------------------------------------------------
if [[ $WITH_AX -eq 1 ]]; then
  cmp=()
  [[ -s "$OUT/wda-tree.json" ]] && cmp=(--compare-wda "$OUT/wda-tree.json")
  if (cd "$ROOT" && cargo run -q -p forge-device --features ax-probe --example ax_probe -- --max 60 --audit \
        --json "$OUT/ax-report.json" "${cmp[@]}") > "$OUT/steps/X1.txt" 2>&1; then
    record X1 optional PASS "Apple accessibility service: stage $(json "$OUT/ax-report.json" 'r["stage_reached"]'), $(json "$OUT/ax-report.json" 'r["summary"]["elements"]') elements, $(json "$OUT/ax-report.json" 'r["summary"]["with_identifier"]') with identifiers (ax-report.json)"
  else
    record X1 optional FAIL "Apple accessibility service: $(json "$OUT/ax-report.json" 'r["error"]') (ax-report.json, steps/X1.txt)"
  fi
else
  skip X1 optional "Apple accessibility service probe (pass --with-ax-probe)"
fi
if [[ $WITH_MCP -eq 1 ]]; then
  printf '%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"acceptance","version":"1"}}}' \
    '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
    '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"forge_device_status","arguments":{}}}' \
    | (cd "$APP" && timeout 120 "$FORGE" mcp) > "$OUT/steps/X2.jsonl" 2> "$OUT/steps/X2.stderr"
  if grep -q '"id":2' "$OUT/steps/X2.jsonl" && grep '"id":2' "$OUT/steps/X2.jsonl" | grep -q '"isError":false'; then
    record X2 optional PASS "forge mcp answered forge_device_status from the phone (steps/X2.jsonl)"
  else
    record X2 optional FAIL "forge mcp: no successful forge_device_status (steps/X2.jsonl)"
  fi
else
  skip X2 optional "forge mcp smoke test (pass --with-mcp)"
fi

# --- Report ----------------------------------------------------------------------
python3 - "$RESULTS" "$OUT" <<'PY'
import json, sys, collections
rows = [l.rstrip("\n").split("\t") for l in open(sys.argv[1]) if l.strip()]
out = sys.argv[2]
json.dump([dict(zip(["id", "kind", "status", "detail"], r)) for r in rows],
          open(f"{out}/results.json", "w"), indent=2)
count = collections.Counter((r[1], r[2]) for r in rows)
with open(f"{out}/results.md", "w") as f:
    f.write("| Step | Kind | Result | Detail |\n|---|---|---|---|\n")
    for r in rows:
        f.write(f"| {r[0]} | {r[1]} | {r[2]} | {r[3].replace('|', '/')} |\n")
    f.write(f"\nMandatory: {count[('mandatory','PASS')]} passed, {count[('mandatory','FAIL')]} failed, {count[('mandatory','SKIP')]} skipped.\n")
print(f"\nMandatory: {count[('mandatory','PASS')]} passed, {count[('mandatory','FAIL')]} failed, {count[('mandatory','SKIP')]} skipped. Report: {out}/results.md")
PY
grep -q $'\tmandatory\t\(FAIL\|SKIP\)\t' "$RESULTS" && exit 1
exit 0
