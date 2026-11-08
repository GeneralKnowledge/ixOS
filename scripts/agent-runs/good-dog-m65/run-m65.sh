#!/usr/bin/env bash
# M6.5 (docs/forge-m6.md): one headless Claude Code agent session takes a real
# Good Dog task (task.txt) through `forge mcp`, in stages, and ends with a run
# record. It works in its own git worktree of this checkout, on a local branch,
# so its change can become a PR afterwards. Stages that don't need the phone
# run first; phone stages are short, so the iPhone is tied up for minutes.
#
#   bash run-m65.sh prep       no phone   worktree, build forge, check claude, warm the iOS build
#   bash run-m65.sh a-change   no phone   make the change, tests, UI test, build (<= 60 min)
#   bash run-m65.sh b-look     PHONE      install, screenshots, largest text, dark, audit, fix (<= 20 min)
#   bash run-m65.sh c-test     PHONE      UI tests until they pass, short explore walk (<= 20 min)
#   bash run-m65.sh d-record   no phone   forge verify --run, check the record (<= 15 min)
#   bash run-m65.sh summary
#
# Afterwards the owner reviews the app on the phone and records acceptance in
# their own terminal: forge verify --project examples/good-dog --accept GD-6 --note "..."
# Phone stages install org.forgemobile.gooddog over the build already there.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
CHECKOUT=${CHECKOUT:-$(cd "$HERE/../../.." && pwd)}  # the Forge checkout this script is in
LAB=${LAB:-$HOME/agentlab/m65}                    # the worktree the agent works in
SRC=$LAB
OUT=${OUT:-$HOME/forge-uitest/m65}
APP=$LAB/examples/good-dog
FORGE=$SRC/target/debug/forge
# Claude Code gives up on an MCP tool call after 60 s unless told otherwise,
# while forge mcp allows a build 30 min and an install 8 min. Without this the
# agent sees a timeout while the build still runs, and starts it again.
export MCP_TOOL_TIMEOUT=${MCP_TOOL_TIMEOUT:-1900000}
TOOLS="mcp__forge,Read,Edit,Write,Glob,Grep,Bash(cargo:*),Bash(ls:*),Bash(cat:*),Bash(git diff:*),Bash(git status:*)"

phone_check() {
  if ! timeout 10 "$FORGE" --json device list | grep -q '"udid"'; then
    echo "no iPhone visible over USB: plug in and unlock, then rerun"; exit 3
  fi
}

# mcp_config phone|nophone: only phone stages get the phone tools and may install.
mcp_config() {
  local allow='"--no-device",'
  [[ $1 == phone ]] && allow='"--allow","install","--bundle-id","org.forgemobile.gooddog",'
  mkdir -p "$OUT"
  cat > "$OUT/mcp.json" <<JSON
{"mcpServers":{"forge":{"command":"$FORGE","args":["mcp",$allow"--root","$LAB"],"env":{"FORGE_WORKSPACE":"$SRC"}}}}
JSON
}

tools() {
  python3 - "$1" <<'PY'
import json, sys
for line in open(sys.argv[1]):
    try: o = json.loads(line)
    except Exception: continue
    if o.get("type") == "assistant":
        for c in o["message"]["content"]:
            if c["type"] == "tool_use" and c["name"].startswith("mcp__forge"):
                print(c["name"].removeprefix("mcp__forge__"), json.dumps(c["input"])[:160])
    if o.get("type") == "result":
        print("RESULT", o.get("subtype"), o.get("num_turns"), "turns", o.get("duration_ms"), "ms", "session", o.get("session_id"))
PY
}

# The session id of the latest stage that has one.
session_id() {
  python3 - "$OUT" <<'PY'
import glob, json, os, sys
sid = None
for path in sorted(glob.glob(os.path.join(sys.argv[1], "*.jsonl")), key=os.path.getmtime):
    for line in open(path):
        try: o = json.loads(line)
        except Exception: continue
        sid = o.get("session_id") or sid
print(sid or "")
PY
}

# stage <name> <phone|nophone> <limit-seconds> <prompt-file>
stage() {
  local name=$1 mode=$2 limit=$3 prompt=$4 start code sid
  local resume=()
  if [[ $name != a-change ]]; then
    sid=$(session_id)
    [[ -n $sid ]] || { echo "no earlier session in $OUT: run a-change first"; exit 1; }
    resume=(--resume "$sid")
  fi
  [[ $mode == phone ]] && phone_check
  mcp_config "$mode"
  # Keep an earlier attempt of the same stage.
  [[ -e $OUT/$name.jsonl ]] && mv "$OUT/$name.jsonl" "$OUT/$name.$(date +%H%M%S).jsonl"
  start=$(date +%s)
  set +e
  (cd "$LAB" && timeout "$limit" claude -p "$(cat "$prompt")" "${resume[@]}" \
    --mcp-config "$OUT/mcp.json" --strict-mcp-config \
    --permission-mode acceptEdits --allowedTools "$TOOLS" \
    --output-format stream-json --verbose \
    > "$OUT/$name.jsonl" 2> "$OUT/$name.stderr.txt")
  code=$?
  set -e
  echo "$name: agent exit $code after $(( $(date +%s) - start )) s$([[ $code == 124 ]] && echo ' (time limit)')"
  tools "$OUT/$name.jsonl" | tee "$OUT/$name.tools.txt"
  cp -r "$APP/.forge/mcp" "$OUT/$name-screens" 2>/dev/null || true
  echo "report: $APP/M65-REPORT.md"
}

case "${1:-}" in
prep)
  if [[ ! -e $LAB ]]; then
    git -C "$CHECKOUT" fetch origin main
    git -C "$CHECKOUT" worktree add -b m65-good-dog "$LAB" origin/main
  fi
  (cd "$SRC" && timeout 900 cargo build -p forge-cli)
  command -v claude >/dev/null || { echo "claude CLI not found"; exit 1; }
  timeout 60 claude -p "reply with the word ok" </dev/null
  # Warm the iOS build of the app as it is now (no phone).
  FORGE_WORKSPACE=$SRC timeout 1800 "$FORGE" build ios --project "$APP"
  mkdir -p "$OUT"
  echo "prep done"
  ;;
a-change) stage a-change nophone 3600 "$HERE/a-change.txt" ;;
b-look)   stage b-look   phone   1200 "$HERE/b-look.txt" ;;
c-test)   stage c-test   phone   1200 "$HERE/c-test.txt" ;;
d-record) stage d-record nophone 900  "$HERE/d-record.txt" ;;
summary)
  for f in "$OUT"/*.tools.txt; do echo "== $f"; grep RESULT "$f" || echo "(no result: stopped or cut off)"; done
  git -C "$LAB" status --short
  ls "$APP/.forge/runs" 2>/dev/null
  ;;
*) sed -n '8,13p' "$0"; exit 2 ;;
esac
