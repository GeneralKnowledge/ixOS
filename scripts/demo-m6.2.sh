#!/usr/bin/env bash
# Forge M6.2 demonstration: exploration aimed at a change, without a phone.
#
# Good Dog is walked as a model (examples/good-dog/uitests/model/): its
# screens are the accessibility trees recorded on the iPhone for the GD1
# journey's visual baselines, plus five screens written by hand from the
# Swift source (marked `synthesized` in the model). The "change" is a real
# file of the app, Sources/AppHost/ExerciseView.swift; the regression is
# planted in the model (Save on the "How did it go?" sheet closes the app).
#
# What it shows, in order:
#   1. forge ui focus: which controls the changed file declares, the ways
#      into them, the UI tests that use them.
#   2. An earlier, plain walk of the app before the change (the map).
#   3. The walk aimed at the change finds the crash, ties it to the changed
#      file, and its replay brings it back.
#   4. The same seed walks the same way again.
#   5. Plain against aimed, over 20 seeds with the same 20-action budget.
#   6. After the fix, the aimed walk rechecks the earlier crash: gone.
#
# A model proves how Forge explores and reports, not how Good Dog behaves:
# every bundle it writes says kind = "fixture", `forge verify` never counts
# it as a device result, and a walk on the phone never learns from it or
# compares with it.
#
#   scripts/demo-m6.2.sh [OUT_DIR]
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
out="${1:-$(mktemp -d -t forge-m6.2-demo.XXXXXX)}"
mkdir -p "$out"
app=examples/good-dog
model="$app/uitests/model"
changed=Sources/AppHost/ExerciseView.swift

cargo build -q -p forge-cli
forge="$root/target/debug/forge"

walk() { # name model seed actions [extra args...]
  local name=$1 file=$2 seed=$3 actions=$4
  shift 4
  "$forge" --json ui explore --project "$app" --fixture "$model/$file" \
    --seed "$seed" --actions "$actions" --out "$out/$name" "$@" >"$out/$name.result.json" 2>"$out/$name.log" || true
}

first_crash() { # explore.json -> step of the first app_closed, or -
  python3 - "$1" <<'PY'
import json, sys
r = json.load(open(sys.argv[1]))
s = [i["step"] for i in r["issues"] if i["issue"] == "app_closed"]
print(s[0] if s else "-")
PY
}

echo "== 1. What the change touches (forge ui focus --changed $changed)"
"$forge" ui focus --project "$app" --changed "$changed"

echo
echo "== 2. An earlier plain walk, before the change (the map later walks learn from)"
walk earlier before.json 7 40 --no-previous --verify 0
python3 - "$out/earlier/explore.json" <<'PY'
import json, sys
r = json.load(open(sys.argv[1]))
print(f"  {len(r['screens'])} screens, {r['actions']} actions, {len(r['issues'])} issues, no crash: {not any(i['issue']=='app_closed' for i in r['issues'])}")
PY

echo
echo "== 3. After the change: the walk aimed at it (seed 7, 20 actions, learning from the earlier walk)"
walk aimed regressed.json 7 20 --changed "$changed" --previous "$out/earlier/explore.json"
python3 - "$out/aimed/explore.json" <<'PY'
import json, sys
r = json.load(open(sys.argv[1]))
c = next(i for i in r["issues"] if i["issue"] == "app_closed")
print(f"  crash found at step {c['step']}: {c['message']}")
print(f"  tie to the change: {c['focus']}, {c['focus_basis']} (read from the source as text, not proof); files: {[f['file'] + ' (' + f['how'] + ')' for f in c['changed_files']]}")
print(f"  confidence: {c['confidence']}; came back on replay: {c['reproduced']}")
print(f"  replay file: {c['repro_file']}; screen tree: {c['evidence']['screen_tree']}")
f = r["focus"]
print(f"  focus: first declared control tried at step {f['first_direct_action']}; not reached: {f['not_reached']}")
PY
echo "  bundle kind: $(python3 -c "import json;print(json.load(open('$out/aimed/evidence.json'))['kind'])")"

echo
echo "== 4. The same seed again walks the same way"
walk aimed-again regressed.json 7 20 --changed "$changed" --previous "$out/earlier/explore.json"
python3 - "$out/aimed/explore.json" "$out/aimed-again/explore.json" <<'PY'
import json, sys
seq = lambda p: [(s["phase"], s["from"], s["action"], s["outcome"]) for s in json.load(open(p))["journal"]]
a, b = seq(sys.argv[1]), seq(sys.argv[2])
print(f"  {len(a)} steps, identical: {a == b}")
sys.exit(0 if a == b else 1)
PY

echo
echo "== 5. Plain against aimed: step the crash was found at, 20 seeds, 20 actions each (- = missed)"
plain=""; aimed=""
for seed in $(seq 1 20); do
  walk "plain-$seed" regressed.json "$seed" 20 --no-previous --verify 0
  walk "aimed-$seed" regressed.json "$seed" 20 --changed "$changed" --no-previous --verify 0
  plain="$plain $(first_crash "$out/plain-$seed/explore.json")"
  aimed="$aimed $(first_crash "$out/aimed-$seed/explore.json")"
done
echo "  plain walk:$plain"
echo "  aimed walk:$aimed   (no earlier walk to learn from)"
found_plain=$(echo $plain | tr ' ' '\n' | grep -vc -- - || true)
found_aimed=$(echo $aimed | tr ' ' '\n' | grep -vc -- - || true)
echo "  found: plain $found_plain/20, aimed $found_aimed/20"
if [ "$found_aimed" -lt "$found_plain" ] || [ "$found_aimed" -lt 18 ]; then
  echo "FAIL: the aimed walk should find the crash at least as often as the plain one, and nearly always" >&2
  exit 1
fi

echo
echo "== 6. After the fix: the aimed walk rechecks the earlier crash"
walk fixed before.json 7 20 --changed "$changed" --previous "$out/aimed/explore.json"
python3 - "$out/fixed/compare.json" <<'PY'
import json, sys
c = json.load(open(sys.argv[1]))
for k in ["gone", "still_present", "new", "not_rechecked"]:
    print(f"  {k}: {[i['issue'] for i in c[k]]}")
if "app_closed" not in [i["issue"] for i in c["gone"]]:
    print("FAIL: the fixed crash should be rechecked and gone", file=sys.stderr)
    sys.exit(1)
PY

echo
echo "Evidence for every walk: $out/<walk>/ (explore.json, focus.json, compare.json, evidence.json, repro/)"
