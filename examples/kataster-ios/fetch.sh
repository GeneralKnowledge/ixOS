#!/usr/bin/env bash
# Fetches Kataster at the commit the trial pins and applies the ixOS trial patch.
# Usage: fetch.sh [DIR]   (default: ./Kataster, which is git-ignored)
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
DIR="${1:-$HERE/Kataster}"
COMMIT=d0a67e79  # "Fix Pause menu blinking", Bevy 0.17.2, 2025-10-22

if [[ ! -d "$DIR/.git" ]]; then
  git clone https://github.com/Bobox214/Kataster "$DIR"
fi
git -C "$DIR" checkout --quiet --detach "$COMMIT"
git -C "$DIR" apply --check "$HERE/patch/0001-ixos-iphone-trial.patch"
git -C "$DIR" apply "$HERE/patch/0001-ixos-iphone-trial.patch"
echo "Kataster $COMMIT with the trial patch is in $DIR"
