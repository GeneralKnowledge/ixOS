#!/usr/bin/env bash
# The macOS CI job (.github/workflows/macos.yml): for each app it runs the
# Swift host tests (XCTest against the app's Rust core) and an unsigned
# `xcodebuild` device build (the only place the iOS SDK compiles our Swift).
# These are the checks Linux cannot do.
#
# Usage:
#   scripts/ci-macos.sh all                 every app
#   scripts/ci-macos.sh affected <base-sha> [head]  only the apps the diff touches
#   scripts/ci-macos.sh <app>...            named apps
#   scripts/ci-macos.sh --list affected <base-sha>   print the selection and stop
#
# Apps: counter weather chat pocket-library good-dog device-lab template services-check.
# "template" is a fresh `forge init` app outside the checkout; "services-check"
# is the native services device-check app (docs/native-services-device-check.md).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

ALL=(counter weather chat pocket-library good-dog device-lab template services-check)

LIST_ONLY=0
if [[ "${1:-}" == "--list" ]]; then
  LIST_ONLY=1
  shift
fi

# The apps a diff touches. A change to one example checks that example. A
# change to the CLI or the templates checks a fresh template app. A change to
# anything else shared (Forge crates, scripts, the workflow) checks the
# template app and Device Lab, which selects every optional native service;
# a change to the services themselves also checks the services check app. The full set runs on main and by hand.
affected() {
  local base="$1" head="${2:-HEAD}" file
  local -A pick=()
  git fetch -q --depth=1 origin "$base" 2>/dev/null || true
  while IFS= read -r file; do
    case "$file" in
      *.md | docs/* | LICENSE* | examples/*-android/* | templates/android-app/* \
        | .github/workflows/android.yml | .github/workflows/screenshots.yml) ;;
      examples/chat-core/* | examples/chat-server/* | examples/chat/*) pick[chat]=1 ;;
      examples/counter/*) pick[counter]=1 ;;
      examples/weather/*) pick[weather]=1 ;;
      examples/pocket-library/*) pick[pocket-library]=1 ;;
      examples/good-dog/* | examples/good-dog-core/*) pick[good-dog]=1 ;;
      examples/device-lab/*) pick[device-lab]=1 ;;
      crates/forge-cli/* | templates/*) pick[template]=1 ;;
      crates/forge-services/* | scripts/make-services-check-app.sh)
        pick[template]=1; pick[device-lab]=1; pick[services-check]=1 ;;
      *) pick[template]=1; pick[device-lab]=1 ;;
    esac
  done < <(git diff --name-only "$base" "$head")
  local app
  for app in "${ALL[@]}"; do
    [[ -n "${pick[$app]:-}" ]] && echo "$app"
  done
  return 0
}

case "${1:-}" in
  all) APPS=("${ALL[@]}") ;;
  affected) mapfile -t APPS < <(affected "${2:?usage: ci-macos.sh affected <base-sha> [head]}" "${3:-HEAD}") ;;
  "") echo "usage: ci-macos.sh all | affected <base-sha> | <app>..." >&2; exit 2 ;;
  *) APPS=("$@") ;;
esac

if [[ $LIST_ONLY -eq 1 ]]; then
  printf '%s\n' "${APPS[@]}"
  exit 0
fi
if [[ ${#APPS[@]} -eq 0 ]]; then
  echo "[ci-macos] Nothing to check."
  exit 0
fi
echo "[ci-macos] Checking: ${APPS[*]}"

xcode_build() {
  xcodebuild build \
    -project "$1" \
    -scheme App \
    -destination 'generic/platform=iOS' \
    -configuration Debug \
    CODE_SIGNING_ALLOWED=NO \
    -quiet
}

group() { echo "::group::$*"; }
endgroup() { echo "::endgroup::"; }

for app in "${APPS[@]}"; do
  case "$app" in
    counter | weather)
      # Their Xcode projects are validated on Linux (rust.yml); the host
      # tests compile their Swift.
      group "$app: Swift host tests"
      ./scripts/test-swift-host.sh "examples/$app"
      endgroup
      ;;
    chat | pocket-library | good-dog | device-lab)
      project="$(find "examples/$app/Xcode" -maxdepth 1 -name '*.xcodeproj' | head -1)"
      group "$app: Swift host tests"
      ./scripts/test-swift-host.sh "examples/$app"
      endgroup
      group "$app: iOS device build (unsigned)"
      xcode_build "$project"
      endgroup
      ;;
    template)
      APP="${RUNNER_TEMP:-$(mktemp -d)}/CiTemplateApp"
      rm -rf "$APP"
      group "template: forge init --path, forge test"
      cargo run -q -p forge-cli -- init CiTemplateApp --platform ios --bundle-id org.example.citemplate --path "$APP"
      cargo run -q -p forge-cli -- test --project "$APP"
      endgroup
      group "template: iOS device build (unsigned)"
      xcode_build "$APP/Xcode/CiTemplateApp.xcodeproj"
      endgroup
      ;;
    services-check)
      APP="${RUNNER_TEMP:-$(mktemp -d)}/ServicesCheck"
      rm -rf "$APP"
      group "services-check: generate, forge test"
      ./scripts/make-services-check-app.sh "$APP"
      cargo run -q -p forge-cli -- test --project "$APP"
      endgroup
      group "services-check: iOS device build (unsigned)"
      xcode_build "$APP/Xcode/ServicesCheck.xcodeproj"
      endgroup
      ;;
    *)
      echo "error: unknown app '$app' (known: ${ALL[*]})" >&2
      exit 2
      ;;
  esac
done

echo "[ci-macos] OK: ${APPS[*]}"
