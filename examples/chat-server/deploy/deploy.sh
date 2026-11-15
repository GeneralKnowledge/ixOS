#!/usr/bin/env bash
# Puts the Chat example's server on one of your Linux machines over SSH, as a
# system service behind Tailscale's HTTPS, so Chat works on your iPhone
# anywhere your tailnet reaches. Run it on a Linux machine with this checkout
# (the laptop), not on the server:
#
#   examples/chat-server/deploy/deploy.sh <ssh host> [https port]
#
# It builds chat-server here (through `forge queue` when there is one, so it
# waits its turn with other builds), copies it and the service file over,
# starts the service, and asks Tailscale on the server to serve it at
# https://<server>.<tailnet>.ts.net:<port> (default 8787). Running it again
# updates the binary and keeps the data. Needs: SSH to the host with sudo,
# Tailscale on the host with HTTPS certificates on for the tailnet, and the
# same CPU architecture here and there. See README.md.
set -euo pipefail

HOST="${1:?usage: deploy.sh <ssh host> [https port]}"
PORT="${2:-8787}"
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
DIR="$ROOT/examples/chat-server/deploy"

here="$(uname -m)"
there="$(ssh "$HOST" uname -m)"
if [[ "$here" != "$there" ]]; then
  echo "error: $HOST is $there and this machine is $here; build on a $there machine" >&2
  exit 1
fi

build=(cargo build --release --locked -p chat-server)
cd "$ROOT"
if command -v forge >/dev/null && forge queue status >/dev/null 2>&1; then
  forge queue run --note "${FORGE_BUILD_LOCK_NOTE:-chat-server deploy}" -- "${build[@]}"
else
  "${build[@]}"
fi

echo "[deploy] copying to $HOST"
scp -q target/release/chat-server "$DIR/chat-server.service" "$HOST:/tmp/"
ssh "$HOST" "set -e
  sudo install -m 0755 /tmp/chat-server /usr/local/bin/chat-server
  sudo install -m 0644 /tmp/chat-server.service /etc/systemd/system/ixos-chat.service
  rm -f /tmp/chat-server /tmp/chat-server.service
  sudo systemctl daemon-reload
  sudo systemctl enable --now ixos-chat.service
  sudo systemctl restart ixos-chat.service
  sudo tailscale serve --bg --https=$PORT http://127.0.0.1:8787 >/dev/null
  sleep 1
  systemctl is-active ixos-chat.service
  # 401: it answers, and wants an account.
  test \"\$(curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:8787/rooms)\" = 401"

name="$(ssh "$HOST" "tailscale status --json" | python3 -c 'import json,sys; print(json.load(sys.stdin)["Self"]["DNSName"].rstrip("."))')"
echo "[deploy] Chat server: https://$name:$PORT"
echo "[deploy] Check it: curl -s https://$name:$PORT/rooms   (401 without an account is right)"
