#!/usr/bin/env bash
# Install the pinned xtool AppImage (toolchain/xtool.env) and refuse it if its
# SHA-256 doesn't match. Used by environment.json; safe to run by hand.
#
#   scripts/install-xtool.sh [DEST]     # default /usr/local/bin/xtool
#
# Only x86_64 has a pinned build. On another architecture it stops and says
# how to get one, rather than falling back to an unpinned download.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=../toolchain/xtool.env
source "$ROOT/toolchain/xtool.env"
DEST="${1:-/usr/local/bin/xtool}"

arch="$(uname -m)"
case "$arch" in
  x86_64) url="$XTOOL_URL_X86_64"; sha="$XTOOL_SHA256_X86_64" ;;
  *)
    echo "[forge] error: no pinned xtool build for $arch. Build one from the fork's ixos branch (Linux/build.sh) and add its URL and SHA-256 to toolchain/xtool.env." >&2
    exit 1
    ;;
esac

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT
echo "[forge] downloading xtool $XTOOL_VERSION ($arch)"
curl -fsSL --retry 3 "$url" -o "$tmp"
got="$(sha256sum "$tmp" | cut -d' ' -f1)"
if [[ "$got" != "$sha" ]]; then
  echo "[forge] error: xtool checksum mismatch: expected $sha, got $got. Not installed." >&2
  exit 1
fi
chmod +x "$tmp"
if [[ -w "$(dirname "$DEST")" ]]; then
  mv "$tmp" "$DEST"
else
  sudo mv "$tmp" "$DEST"
fi
trap - EXIT
echo "[forge] installed xtool $XTOOL_VERSION at $DEST (sha256 verified)"
