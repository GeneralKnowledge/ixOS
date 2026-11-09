#!/usr/bin/env bash
# The ForgeKit release refuses any destination but a clean clone of the
# ForgeKit repository, and releases exactly the committed forgekit/.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
export FORGEKIT_ALLOW_UNMERGED=1

fail() { echo "FAIL: $*" >&2; exit 1; }
release() { "$ROOT/scripts/forgekit.sh" release "$@" >"$TMP/out" 2>&1; }

# A stand-in for the ForgeKit repository, and a clone of it.
git init -q --bare "$TMP/ForgeKit.git"
git clone -q "$TMP/ForgeKit.git" "$TMP/clone" 2>/dev/null
export FORGEKIT_REPOSITORY="$TMP/ForgeKit.git"

# Not a git clone: refused, untouched.
mkdir "$TMP/plain" && echo keep >"$TMP/plain/file"
release "$TMP/plain" && fail "released into a plain directory"
[[ -f "$TMP/plain/file" ]] || fail "plain directory was changed"

# A clone of some other repository: refused, untouched.
git init -q --bare "$TMP/Other.git"
git clone -q "$TMP/Other.git" "$TMP/other" 2>/dev/null
echo keep >"$TMP/other/file"
release "$TMP/other" && fail "released into another repository"
grep -q "refusing to touch it" "$TMP/out" || fail "wrong refusal: $(cat "$TMP/out")"
[[ -f "$TMP/other/file" ]] || fail "other repository was changed"

# The right clone with local changes: refused.
echo dirty >"$TMP/clone/dirty"
release "$TMP/clone" && fail "released over uncommitted changes"
rm "$TMP/clone/dirty"

# The right, clean clone: released, with only committed forgekit/ files.
release "$TMP/clone" || fail "release failed: $(cat "$TMP/out")"
diff <(cd "$ROOT" && git ls-tree -r --name-only HEAD:forgekit | sort) \
  <(cd "$TMP/clone" && git ls-files | grep -v '^RELEASE$' | sort) || fail "release content differs from forgekit/"
git -C "$TMP/clone" rev-parse -q --verify refs/tags/0.3.0 >/dev/null || fail "no tag"

# The same version again: refused.
release "$TMP/clone" && fail "released the same version twice"
echo "[forge] ForgeKit release safety checks pass."
