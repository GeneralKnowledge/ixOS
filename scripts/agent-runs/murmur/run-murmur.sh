#!/usr/bin/env bash
# Murmur (M2 in docs/forge-mcp.md): one headless Claude Code agent builds,
# installs, inspects and fixes a local-only social app on the iPhone through
# `forge mcp`, then takes two follow-up prompts in the same session.
# Run on the laptop with the phone plugged in and unlocked.
#
#   prep        (no phone)  bash run-murmur.sh prep
#   run         (phone)     bash run-murmur.sh run          # up to 90 min, unattended
#   followup N  (phone)     bash run-murmur.sh followup 1   # then 2; up to 45 min each
#   summary                 bash run-murmur.sh summary
#
# Needs: claude (Claude Code CLI) on PATH and signed in; the WDA runner
# installed and "Enable UI Automation" on; the agentlab slot (Murmur installs
# over whatever agent app is there, Tipster today).
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

mcp_config() {
  mkdir -p "$LAB" "$OUT"
  cat > "$OUT/mcp.json" <<JSON
{"mcpServers":{"forge":{"command":"$FORGE","args":["mcp","--root","$LAB","--allow","install","--bundle-id","org.forgemobile.agentlab"],"env":{"FORGE_WORKSPACE":"$SRC"}}}}
JSON
}

# agent <name> <limit-seconds> <prompt-file> [claude args...]
agent() {
  local name=$1 limit=$2 prompt=$3; shift 3
  local start code
  start=$(date +%s)
  set +e
  (cd "$LAB" && timeout "$limit" claude -p "$(cat "$prompt")" "$@" \
    --mcp-config "$OUT/mcp.json" --strict-mcp-config \
    --permission-mode acceptEdits --allowedTools "$TOOLS" \
    --output-format stream-json --verbose \
    > "$OUT/$name.jsonl" 2> "$OUT/$name.stderr.txt")
  code=$?
  set -e
  echo "$name: agent exit $code after $(( $(date +%s) - start )) s"
  tools "$OUT/$name.jsonl" | tee "$OUT/$name.tools.txt"
  cp -r "$LAB/Murmur/.forge/mcp" "$OUT/$name-screens" 2>/dev/null || true
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

session_id() {
  python3 - "$OUT/run.jsonl" <<'PY'
import json, sys
sid = None
for line in open(sys.argv[1]):
    try: o = json.loads(line)
    except Exception: continue
    sid = o.get("session_id") or sid
print(sid or "")
PY
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
  timeout 30 "$FORGE" --json device list || true
  echo "prep done"
  ;;
run)
  [[ ! -e $LAB/Murmur ]] || { echo "$LAB/Murmur exists; move it away first"; exit 1; }
  phone_check
  mcp_config
  cp "$HERE"/*.txt "$OUT/"
  agent run 5400 "$HERE/prompt.txt"
  echo "report: $LAB/Murmur/REPORT.md; transcripts: $OUT"
  ;;
followup)
  n=${2:?usage: followup 1|2}
  sid=$(session_id)
  [[ -n $sid ]] || { echo "no session id in $OUT/run.jsonl"; exit 1; }
  phone_check
  mcp_config
  agent "followup-$n" 2700 "$HERE/followup-$n.txt" --resume "$sid"
  ;;
summary)
  for f in "$OUT"/*.tools.txt; do echo "== $f"; grep RESULT "$f" || true; done
  ls "$LAB/Murmur/uitests" 2>/dev/null
  ;;
*) echo "usage: $0 prep|run|followup N|summary"; exit 2;;
esac
