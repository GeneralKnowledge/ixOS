#!/usr/bin/env bash
# The iOS device build check, on Linux with xtool: every app's SwiftUI
# compiled and linked against the iOS SDK, unsigned, with no iPhone and no
# Apple sign-in. This is what the macOS workflow's `xcodebuild` builds proved,
# done on a Linux machine that has xtool and its Darwin SDK (Reuben's laptop),
# in minutes instead of a ~35 minute macOS queue. See docs/ios-build-check.md.
#
# Usage:
#   scripts/check-ios-xtool.sh all                       every app
#   scripts/check-ios-xtool.sh affected <base> [head]     only the apps the diff touches
#   scripts/check-ios-xtool.sh <app>...                   named apps
#   scripts/check-ios-xtool.sh --list affected <base>     print the selection and stop
#   scripts/check-ios-xtool.sh --ref <git-ref> <selection>...
#       check a branch or commit without touching this checkout: it is fetched
#       from origin and built in a reused worktree under
#       ${FORGE_IOS_CHECK_DIR:-~/.cache/forge/ios-check}, so later runs are warm.
#
# Apps: every examples/<app> with an xtool.yml, plus "template" (a fresh
# `forge init` app), "minimal" (`forge init --minimal`) and "services-check"
# (scripts/make-services-check-app.sh). Each build gets
# ${FORGE_IOS_CHECK_TIMEOUT:-1200} seconds. A failure doesn't stop the run: every
# selected app is built, the summary lists each result, and the exit status is
# 1 if any failed. Full logs are kept in the log directory it prints.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

usage() {
  sed -n '8,16p' "$0" | sed 's/^# \{0,1\}//' >&2
  exit 2
}

# --ref: build another commit in a worktree, by running this script from there.
if [[ "${1:-}" == "--ref" ]]; then
  ref="${2:?usage: check-ios-xtool.sh --ref <git-ref> <selection>...}"
  shift 2
  [[ $# -gt 0 ]] || usage
  base_dir="${FORGE_IOS_CHECK_DIR:-$HOME/.cache/forge/ios-check}"
  wt="$base_dir/ixOS"
  mkdir -p "$base_dir"
  git fetch -q origin "$ref" 2>/dev/null || git fetch -q origin
  sha="$(git rev-parse --verify -q "origin/$ref^{commit}" || git rev-parse --verify "$ref^{commit}")"
  if [[ ! -e "$wt/.git" ]]; then
    git worktree prune
    git worktree add -q --detach "$wt" "$sha"
  else
    # Generated files a build rewrote must not block the next checkout.
    git -C "$wt" reset -q --hard
    git -C "$wt" checkout -q --detach "$sha"
  fi
  echo "[ios-check] $ref is $(git -C "$wt" log -1 --format='%h %s')"
  exec "$wt/scripts/check-ios-xtool.sh" "$@"
fi

ALL=()
for yml in examples/*/xtool.yml; do
  ALL+=("$(basename "$(dirname "$yml")")")
done
ALL+=(template minimal services-check)

LIST_ONLY=0
if [[ "${1:-}" == "--list" ]]; then
  LIST_ONLY=1
  shift
fi

has() { [[ " ${ALL[*]} " == *" $1 "* ]]; }

# The apps a diff touches. A change to one example checks that example. A
# change to ForgeKit (ForgeUI, ForgeAppKit, the host) checks the apps that
# are only ForgeKit (the showcase, Checklist) and the template. A change to
# the CLI or the templates checks the starters. Anything else shared (Forge
# crates, scripts) checks the template and Device Lab, which selects every
# optional native service; a change to the services also checks the services
# check app. Docs, Android and CI-only files check nothing.
affected() {
  local base="$1" head="${2:-HEAD}" file name
  local -A pick=()
  local files
  git fetch -q --depth=1 origin "$base" 2>/dev/null || true
  # A diff that can't be computed must not look like "nothing changed".
  if ! files="$(git diff --name-only "$base" "$head")"; then
    echo "[ios-check] warning: cannot diff $base..$head; checking every app" >&2
    printf '%s\n' "${ALL[@]}"
    return 0
  fi
  while IFS= read -r file; do
    [[ -z "$file" ]] && continue
    case "$file" in
      *.md | docs/* | LICENSE* | examples/*-android/* | templates/android-app/* | .github/*) ;;
      examples/chat-core/* | examples/chat-server/*) pick[chat]=1 ;;
      examples/good-dog-core/*) pick[good-dog]=1 ;;
      forgekit/*) pick[forgeui-showcase]=1; pick[checklist]=1; pick[template]=1 ;;
      crates/forge-cli/* | templates/*) pick[template]=1; pick[minimal]=1 ;;
      crates/forge-services/* | scripts/make-services-check-app.sh)
        pick[template]=1; pick[device-lab]=1; pick[services-check]=1 ;;
      examples/*/*)
        name="${file#examples/}"; name="${name%%/*}"
        if has "$name"; then pick[$name]=1
        else pick[template]=1; pick[device-lab]=1; fi ;;
      *) pick[template]=1; pick[device-lab]=1 ;;
    esac
  done <<< "$files"
  local app
  for app in "${ALL[@]}"; do
    [[ -n "${pick[$app]:-}" ]] && echo "$app"
  done
  return 0
}

case "${1:-}" in
  all) APPS=("${ALL[@]}") ;;
  affected) mapfile -t APPS < <(affected "${2:?usage: check-ios-xtool.sh affected <base> [head]}" "${3:-HEAD}") ;;
  "" | -h | --help) usage ;;
  *) APPS=("$@") ;;
esac

if [[ $LIST_ONLY -eq 1 ]]; then
  printf '%s\n' "${APPS[@]}"
  exit 0
fi
if [[ ${#APPS[@]} -eq 0 ]]; then
  echo "[ios-check] Nothing to check."
  exit 0
fi
for app in "${APPS[@]}"; do
  has "$app" || { echo "error: unknown app '$app' (known: ${ALL[*]})" >&2; exit 2; }
done

if ! command -v xtool >/dev/null; then
  echo "error: xtool is not installed; this check needs xtool and its Darwin SDK (docs/linux-setup.md)" >&2
  exit 1
fi

TIMEOUT="${FORGE_IOS_CHECK_TIMEOUT:-1200}"
LOGS="$(mktemp -d "${TMPDIR:-/tmp}/forge-ios-check.XXXXXX")"
WORK="$LOGS/apps"
mkdir -p "$WORK"
export FORGE_WORKSPACE="$ROOT"

echo "[ios-check] $(git log -1 --format='%h %s' 2>/dev/null || echo 'not a git checkout')"
echo "[ios-check] $(xtool --version 2>/dev/null | head -1), $(swift --version 2>/dev/null | head -1)"
echo "[ios-check] Checking: ${APPS[*]}"
echo "[ios-check] Logs: $LOGS"

cargo build -q -p forge-cli
FORGE="$ROOT/target/debug/forge"

# One app's build: forge apps through `forge build ios` (the app's Rust for
# aarch64-apple-ios, then `xtool dev build`, unsigned); the ForgeUI showcase,
# which has no Rust core, with xtool directly.
build_app() {
  local app="$1" dir
  case "$app" in
    forgeui-showcase)
      (cd examples/forgeui-showcase && xtool dev build) ;;
    template)
      dir="$WORK/CiTemplateApp"
      "$FORGE" init CiTemplateApp --platform ios --bundle-id org.example.citemplate --path "$dir"
      "$FORGE" build ios --project "$dir" ;;
    minimal)
      dir="$WORK/CiMinimalApp"
      "$FORGE" init CiMinimalApp --minimal --bundle-id org.example.ciminimal --path "$dir"
      "$FORGE" build ios --project "$dir" ;;
    services-check)
      dir="$WORK/ServicesCheck"
      ./scripts/make-services-check-app.sh "$dir"
      "$FORGE" build ios --project "$dir" ;;
    *)
      "$FORGE" build ios --project "examples/$app" ;;
  esac
}

declare -A RESULT=() SECONDS_TAKEN=()
FAILED=0
for app in "${APPS[@]}"; do
  echo "[ios-check] $app ..."
  start=$SECONDS
  # `timeout` needs a program, so the function runs in a child bash.
  if timeout "$TIMEOUT" bash -c "set -euo pipefail; $(declare -f build_app); FORGE='$FORGE' WORK='$WORK'; build_app '$app'" \
      >"$LOGS/$app.log" 2>&1; then
    RESULT[$app]=ok
  else
    status=$?
    if [[ $status -eq 124 ]]; then RESULT[$app]="TIMED OUT after ${TIMEOUT}s"; else RESULT[$app]="FAILED"; fi
    FAILED=$((FAILED + 1))
    echo "---- last 40 lines of $LOGS/$app.log"
    tail -n 40 "$LOGS/$app.log"
    echo "----"
  fi
  SECONDS_TAKEN[$app]=$((SECONDS - start))
done

echo
echo "[ios-check] Results ($(git log -1 --format=%h 2>/dev/null || echo '?')):"
for app in "${APPS[@]}"; do
  printf '  %-18s %-24s %4ss\n' "$app" "${RESULT[$app]}" "${SECONDS_TAKEN[$app]}"
done
echo "[ios-check] Logs: $LOGS"
# The builds above are what `forge run ios` installs: in this checkout it
# rebuilds only what changed (same configuration: debug Swift, release Rust),
# then signs and installs. Nothing here needs the phone.
for app in "${APPS[@]}"; do
  if [[ "${RESULT[$app]}" == ok && -f "examples/$app/forge.toml" ]]; then
    echo "[ios-check] Install $app from this build: $FORGE run ios --project $ROOT/examples/$app"
  fi
done
if [[ $FAILED -gt 0 ]]; then
  echo "[ios-check] $FAILED of ${#APPS[@]} failed"
  exit 1
fi
echo "[ios-check] OK: ${APPS[*]}"
