#!/usr/bin/env bash
# Creates the local AI check app (spike L0 in docs/local-ai.md): a fresh
# `forge init` iOS app whose one screen tries Apple Foundation Models and
# CoreML-LLM on a real iPhone, records memory, speed and stopping, and plays
# the test characters through the Rust prompt builder. Install it with
# `forge run ios` from Linux (runbook: docs/local-ai-check.md).
#
# Usage: scripts/make-ai-check-app.sh [dir]   (default: ~/forge-ai-check)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP="${1:-$HOME/forge-ai-check}"
COREML_LLM_VERSION="1.9.0"
if [[ -e "$APP" ]]; then
  echo "error: $APP already exists; remove it or pass another directory" >&2
  exit 1
fi

cargo build -q --manifest-path "$ROOT/Cargo.toml" -p forge-cli
export FORGE_WORKSPACE="$ROOT"
"$ROOT/target/debug/forge" init AICheck --platform ios \
  --bundle-id "${FORGE_CHECK_BUNDLE_ID:-org.example.aicheck}" --path "$APP"

cat "$ROOT/scripts/ai-check/ai_check.rs" >>"$APP/rust/src/lib.rs"
cp "$ROOT/scripts/ai-check/ContentView.swift" "$APP/Sources/AppHost/ContentView.swift"

# CoreML-LLM needs iOS 18 and macOS 15. forge config sync writes the iOS
# version into Package.swift; the macOS one and the Xcode project are ours.
sed -i.bak 's/^minimum_ios = "17.0"/minimum_ios = "18.0"/' "$APP/forge.toml"
sed -i.bak 's/\.macOS(\.v14)/.macOS(.v15)/' "$APP/Package.swift"
sed -i.bak 's/IPHONEOS_DEPLOYMENT_TARGET = 17.0;/IPHONEOS_DEPLOYMENT_TARGET = 18.0;/' \
  "$APP/Xcode/AICheck.xcodeproj/project.pbxproj"

# Add CoreML-LLM (and its tokenizers) to the AppHost target. The check screen
# is a throwaway, so it builds in Swift 5 mode: CoreML-LLM's types are not
# Sendable, and strict concurrency would only get in the way of the spike.
perl -0pi -e '
  s{\n    targets: \[}{\n    dependencies: [\n        .package(url: "https://github.com/john-rocky/CoreML-LLM", exact: "'"$COREML_LLM_VERSION"'"),\n        .package(url: "https://github.com/huggingface/swift-transformers", "1.0.0"..<"2.0.0"),\n    ],\n    targets: [};
  s{dependencies: \["ForgeFFI"\],\n            path: "Sources/AppHost"}{dependencies: [\n                "ForgeFFI",\n                .product(name: "CoreMLLLM", package: "CoreML-LLM"),\n                .product(name: "Tokenizers", package: "swift-transformers"),\n            ],\n            path: "Sources/AppHost",\n            swiftSettings: [.swiftLanguageMode(.v5)]};
' "$APP/Package.swift"
grep -q 'CoreML-LLM' "$APP/Package.swift" || { echo "error: could not add CoreML-LLM to Package.swift" >&2; exit 1; }
grep -q 'swiftLanguageMode(.v5)\]' "$APP/Package.swift" || { echo "error: could not patch the AppHost target" >&2; exit 1; }
rm -f "$APP"/*.bak "$APP"/Xcode/AICheck.xcodeproj/*.bak

# CoreML-LLM's Qwen3.5 generator cannot be stopped: its decode loop has no
# cancellation check, so after Stop the phone keeps computing tokens nobody
# reads. The check app builds its own copy (MIT, same tag, checksum pinned)
# with a stop check before every prefill and decode step, under another
# name, so stopping is real and measurable (docs/local-ai.md, L0).
QWEN_SRC="https://raw.githubusercontent.com/john-rocky/CoreML-LLM/v$COREML_LLM_VERSION/Sources/CoreMLLLM/Qwen35MLKVGenerator.swift"
QWEN_SHA256="53352aa8b6c90b3544ab8e82d874d2be93e1683546058d2309a528d738411bf0"
QWEN_OUT="$APP/Sources/AppHost/StoppableQwen35Generator.swift"
curl -sSfL "$QWEN_SRC" -o "$QWEN_OUT.orig"
echo "$QWEN_SHA256  $QWEN_OUT.orig" | sha256sum -c --quiet - || {
  echo "error: $QWEN_SRC changed; check the patch below against it" >&2
  exit 1
}
{
  printf '%s\n' \
    "// Copied by scripts/make-ai-check-app.sh from CoreML-LLM $COREML_LLM_VERSION" \
    "// (https://github.com/john-rocky/CoreML-LLM, MIT License, Copyright (c) the" \
    "// CoreML-LLM authors), renamed, with a stop check added to the prefill and" \
    "// decode loops. Throwaway spike code; see docs/local-ai.md." \
    ""
  perl -pe '
    s/^public final class Qwen35MLKVGenerator \{/public final class StoppableQwen35Generator {\n    \/\/\/ Checked before every step; true ends the generation (ixOS patch).\n    \@ObservationIgnored public var shouldStop: (() -> Bool)?/;
    s/^(\s*)for \(t, tok\) in inputIds\.enumerated\(\) \{$/$&\n$1    try Task.checkCancellation()\n$1    if shouldStop?() == true { throw CancellationError() }/;
    s/^(\s*)for step in 0\.\.<maxNewTokens \{$/$&\n$1    if Task.isCancelled || shouldStop?() == true { break }/;
  ' "$QWEN_OUT.orig"
} >"$QWEN_OUT"
rm "$QWEN_OUT.orig"
[[ "$(grep -c 'shouldStop?() == true' "$QWEN_OUT")" == 2 ]] || {
  echo "error: could not patch the Qwen3.5 generator" >&2
  exit 1
}

# Rewrite Info.plist, xtool.yml and Package.swift's iOS version from forge.toml.
"$ROOT/target/debug/forge" config sync --project "$APP"

cargo test -q --manifest-path "$APP/rust/Cargo.toml"
"$ROOT/target/debug/forge" bindings generate --project "$APP"
echo
echo "[forge] Local AI check app ready in $APP"
echo "[forge] Next: forge run ios --project $APP   (iPhone plugged in and unlocked)"
