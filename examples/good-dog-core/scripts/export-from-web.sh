#!/usr/bin/env bash
# Re-exports the bundled content and re-records the TypeScript engine's plans
# from a Good Dog web app checkout. Needs Node 22.6+ (type stripping); no npm
# install. Run it after the web app's plan engine or exercises change, then
# check that `cargo test -p good-dog-core` still passes before porting the change.
# Usage: examples/good-dog-core/scripts/export-from-web.sh /path/to/Good-Dog
set -euo pipefail

WEB="${1:?usage: $0 /path/to/Good-Dog}"
CRATE="$(cd "$(dirname "$0")/.." && pwd)"
LIB="$WEB/src/lib"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

mkdir -p "$WORK/coaching" "$WORK/dog-training/content"
cp "$LIB"/coaching/{index,types,policy,progression,plan-generator,welfare}.ts "$WORK/coaching/"
cp "$LIB"/domains/dog-training/{types,policy,why-today,golden-scenarios}.ts "$WORK/dog-training/"
cp "$LIB"/domains/dog-training/content/{exercises,learning-objectives}.ts "$WORK/dog-training/content/"

# Node runs TypeScript only with explicit .ts imports and no path aliases.
# Plan item ids come from nanoid; the tests never look at them.
sed -i -E 's#from "\./([a-z-]+)"#from "./\1.ts"#; s#import \{ nanoid \} from "nanoid";#const nanoid = () => "";#' "$WORK"/coaching/*.ts
sed -i -E 's#@/lib/coaching#../coaching/index.ts#; s#from "\./(content/)?([a-z-]+)"#from "./\1\2.ts"#' "$WORK"/dog-training/*.ts
sed -i -E 's#from "\.\./types"#from "../types.ts"#' "$WORK"/dog-training/content/*.ts

cp "$LIB/domains/dog-training/golden-plans.fixture.json" "$CRATE/tests/fixtures/"
node --experimental-strip-types --no-warnings "$CRATE/scripts/export-from-web.mjs" "$WORK" "$CRATE"
echo "Exported from $(git -C "$WEB" rev-parse --short HEAD 2>/dev/null || echo "$WEB"). Record that commit in $CRATE/content/SOURCE.md."
