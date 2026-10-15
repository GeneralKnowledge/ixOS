# ADR 001: iOS Rust library packaging

## Status

Accepted (2026-10-15)

## Context

Forge Mobile must link a Rust `staticlib` into a SwiftPM iOS app built with xtool on Linux. Options:

1. SwiftPM target `linkerSettings` + staged `libforge_ffi.a`
2. XCFramework via `xcodebuild -create-xcframework`
3. SwiftPM artifact bundle (`staticLibrary`)

## Decision

Use **(1) staged static library + linkerSettings** for **device-only** (`aarch64-apple-ios`) builds.

## Rationale

- No `xcodebuild` on Linux in the xtool workflow.
- Single-arch device slice avoids `lipo` for v1.
- UniFFI generates C headers/modulemap alongside Swift; a dedicated `ForgeFfiC` target exposes headers to SwiftPM.

## Consequences

- Simulator support requires additional slices and likely XCFramework or artifact bundle — tracked when `ios_simulator = true` is implemented.
- Consumers must run `./scripts/build-rust-ios.sh` (or `forge build ios`) before `xtool dev`.

## Fallback

If SwiftPM rejects unsafe linker flags on iOS cross-builds, migrate to **(3) artifact bundle** with the same `.a` and headers under `ForgeFFI.artifactbundle/`.
