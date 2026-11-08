#!/usr/bin/env bash
# Murmur (M2 in docs/forge-mcp.md): one headless Claude Code agent session
# builds a local-only social app through `forge mcp`, in stages. Stages that
# don't need the phone run first and as long as they need; phone stages are
# short, so the iPhone is only tied up for minutes at a time, and Forge can be
# fixed between stages (rebuild forge, then resume).
#
#   bash run-murmur.sh prep        no phone   build forge, check claude, warm the iOS build
#   bash run-murmur.sh a-build     no phone   create, write, check and build the app (<= 60 min)
#   bash run-murmur.sh b-look      PHONE      install, screenshots, tree, largest text, fix (<= 20 min)
#   bash run-murmur.sh c-test      PHONE      run the UI test until it passes (<= 15 min)
#   bash run-murmur.sh change-1    no phone   "the timeline feels cramped" (<= 30 min)
#   bash run-murmur.sh check-1     PHONE      install, look, UI test (<= 15 min)
#   bash run-murmur.sh change-2    no phone   bookmarks (<= 30 min)
#   bash run-murmur.sh check-2     PHONE      install, look, UI test (<= 15 min)
#   bash run-murmur.sh summary
#
# Every stage after a-build resumes the same agent session. A stage can be
# rerun (after a Forge fix, say); it resumes the latest session again.
# Phone stages need the WDA runner installed and "Enable UI Automation" on;
# Murmur installs over whatever agent app holds org.forgemobile.agentlab.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
SRC=${SRC:-$(cd "$HERE/../../.." && pwd)}   # the Forge checkout this script is in
LAB=${LAB:-$HOME/agentlab}                  # folder the agent works in
OUT=${OUT:-$HOME/forge-uitest/murmur-m2}
FORGE=$SRC/target/debug/forge
# Claude Code gives up on an MCP tool call after 60 s unless told otherwise,
# while forge mcp allows a build 30 min and an install 8 min. Without this the
# agent sees a timeout while the build still runs, and starts it again.
export MCP_TOOL_TIMEOUT=${MCP_TOOL_TIMEOUT:-1900000}
TOOLS="mcp__forge,Read,Edit,Write,Glob,Grep,Bash(cargo:*),Bash(ls:*),Bash(cat:*)"

phone_check() {
  if ! timeout 10 "$FORGE" --json device list | grep -q '"udid"'; then
    echo "no iPhone visible over USB: plug in and unlock, then rerun"; exit 3
  fi
}

# mcp_config phone|nophone: only phone stages may install.
mcp_config() {
  local allow=""
  [[ $1 == phone ]] && allow='"--allow","install","--bundle-id","org.forgemobile.agentlab",'
  mkdir -p "$LAB" "$OUT"
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
  if [[ $name != a-build ]]; then
    sid=$(session_id)
    [[ -n $sid ]] || { echo "no earlier session in $OUT: run a-build first"; exit 1; }
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
  cp -r "$LAB/Murmur/.forge/mcp" "$OUT/$name-screens" 2>/dev/null || true
  echo "report: $LAB/Murmur/REPORT.md"
}

case "${1:-}" in
prep)
  (cd "$SRC" && timeout 900 cargo build -p forge-cli)
  command -v claude >/dev/null || { echo "claude CLI not found"; exit 1; }
  timeout 60 claude -p "reply with the word ok" </dev/null
  # Warm the cargo cache the agent's first build needs (no phone).
  rm -rf "$OUT/warm" && mkdir -p "$OUT"
  FORGE_WORKSPACE=$SRC timeout 900 "$FORGE" init Warm --bundle-id org.forgemobile.agentlab --path "$OUT/warm"
  FORGE_WORKSPACE=$SRC timeout 900 "$FORGE" build ios --project "$OUT/warm"
  echo "prep done"
  ;;
a-build)
  [[ ! -e $LAB/Murmur ]] || { echo "$LAB/Murmur exists; move it away first"; exit 1; }
  stage a-build nophone 3600 "$HERE/a-build.txt" ;;
b-look)   stage b-look   phone   1200 "$HERE/b-look.txt" ;;
c-test)   stage c-test   phone   900  "$HERE/c-test.txt" ;;
change-1) stage change-1 nophone 1800 "$HERE/change-1.txt" ;;
check-1)  stage check-1  phone   900  "$HERE/check.txt" ;;
change-2) stage change-2 nophone 1800 "$HERE/change-2.txt" ;;
check-2)  stage check-2  phone   900  "$HERE/check.txt" ;;
summary)
  for f in "$OUT"/*.tools.txt; do echo "== $f"; grep RESULT "$f" || echo "(no result: stopped or cut off)"; done
  ls "$LAB/Murmur/uitests" 2>/dev/null
  ;;
*) sed -n '8,16p' "$0"; exit 2 ;;
esac
