#!/usr/bin/env bash
# Checks that every relative link in the repository's Markdown points at a
# file or folder that exists, so a moved or deleted page can't leave dead
# links behind (docs/areas.md). Links to the web and to anchors in the same
# page are not checked. docs/ledger/page.md is a template rendered into
# docs/capability-ledger.md, so its links are checked there.
#
# Usage: scripts/check-doc-links.sh   (make validate, CI)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
git ls-files -z '*.md' | exec python3 -I -c '
import pathlib, re, sys
link = re.compile(r"\]\(([^)\s]+)\)")
bad = 0
for name in sys.stdin.read().split("\0"):
    if not name or name == "docs/ledger/page.md":
        continue
    path = pathlib.Path(name)
    if not path.exists():  # deleted in the working tree, not yet committed
        continue
    text = path.read_text(encoding="utf-8", errors="replace")
    # Code blocks and inline code can hold link-shaped text that is not a link.
    text = re.sub(r"```.*?```", "", text, flags=re.S)
    text = re.sub(r"`[^`\n]*`", "", text)
    for m in link.finditer(text):
        target = m.group(1).split("#", 1)[0]
        if not target or re.match(r"^[a-z][a-z0-9+.-]*:", target):
            continue
        if not (path.parent / target).exists():
            print(f"error: {name}: dead link to {target}", file=sys.stderr)
            bad += 1
if bad:
    sys.exit(1)
print("doc links ok")
'
