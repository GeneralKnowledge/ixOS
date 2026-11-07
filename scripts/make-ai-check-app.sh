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

# Rewrite Info.plist, xtool.yml and Package.swift's iOS version from forge.toml.
"$ROOT/target/debug/forge" config sync --project "$APP"

cargo test -q --manifest-path "$APP/rust/Cargo.toml"
"$ROOT/target/debug/forge" bindings generate --project "$APP"
echo
echo "[forge] Local AI check app ready in $APP"
echo "[forge] Next: forge run ios --project $APP   (iPhone plugged in and unlocked)"
