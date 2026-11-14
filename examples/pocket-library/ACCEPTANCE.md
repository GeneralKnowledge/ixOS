# Pocket Library acceptance report

What has been checked, where, and what is still to do. Results are kept apart by kind: **automated** (runs in CI on every PR), **local build** (run by hand on a development machine), and **physical device** (an iPhone). The device steps are now part of the framework-wide runbook, [docs/integration/iphone-acceptance.md](../../docs/integration/iphone-acceptance.md) (section E); record results there and in the table below. Nothing here was tested on the iOS Simulator, which Forge does not support. (macOS CI renders screenshots in the simulator for UI review only, `scripts/pocket-library-screenshots.sh`; they are not acceptance results.)

Status as of 2026-11-07. A step is marked done only when it was actually performed.

## Toolchain

| Component | Version | Where it was used |
|-----------|---------|-------------------|
| Rust | 1.97.0 (`rust-toolchain.toml`) | Linux (cloud dev environment); CI |
| UniFFI / uniffi-bindgen | 0.32.2 | Linux; CI |
| xtool | 1.24.0-ixos.1 (patched, GeneralKnowledge/xtool `ixos` branch) | Linux (Reuben's laptop), debug build |
| Swift (Linux) | 6.2.3 (from xtool's version warning) | Linux (Reuben's laptop) |
| Darwin SDK | iOS 26.0 (recorded in the built app's `LC_BUILD_VERSION`) | Linux (Reuben's laptop) |
| Xcode (CI) | `macos-15` runner default, printed in the CI log | CI |

## The workflow

| # | Step | Kind | Status | Evidence |
|---|------|------|--------|----------|
| 1 | Generate the app: `forge init PocketLibrary --platform ios --bundle-id org.forgemobile.pocketlibrary --path examples/pocket-library` | local build (Linux) | Done | The starter was generated, then replaced screen by screen; `forge.toml`, `Package.swift`, `xtool.yml` and the Xcode project are the generated ones |
| 2 | Build and test its Rust crate on Linux | local build + automated | Done | 45 tests pass; clippy clean; `scripts/check-app-crates.sh` runs them in CI |
| 2b | Rust crate compiles for `aarch64-apple-ios` | local build (Linux) | Type-checked only | `cargo check --target aarch64-apple-ios` with stub C headers for `ring` (no Darwin SDK in the cloud environment). The real build happens in step 5 |
| 3 | Generate the Swift bindings | local build + automated | Done | `forge bindings generate --project`; CI fails if the committed `Generated/` drifts from the crate |
| 4 | Compile the SwiftUI host | automated (macOS CI) | Done | XCTest suite (7 tests against the real Rust library, `test-swift-host.sh`) and an unsigned `xcodebuild` of the iOS app pass on PR #33 (commit 66144a5). The first compile found three host problems, fixed: Rust names clashing with `Foundation.SortOrder` and `SwiftUI.Settings`, `@retroactive` on same-package types, a non-`Sendable` generic result |
| 5 | Build the iOS app with xtool on Linux (debug and release) | local build (Linux) | Debug **done**; release not yet | 2026-11-07 on Reuben's laptop with the patched xtool, from this branch before it was integrated with main: clean debug build in 2m11s, iOS SDK 26.0 recorded |
| 6 | Install and launch on a physical iPhone | physical device | **Not yet done** | |
| 7 | Add, edit and delete books | physical device | **Not yet done** | Covered on the Mac host by XCTest (step 4) |
| 8 | Restart the app and check persistence | physical device | **Not yet done** | Covered in Rust (`everything_survives_a_relaunch`) and XCTest |
| 9 | Catalogue lookup and failure handling (airplane mode) | physical device | **Not yet done** | Parsing and every failure type are covered by Rust tests on canned responses; openlibrary.org is not reachable from the cloud environment, so no live request has been made yet |
| S1 | Chapter summaries, planned in Rust: parts fit the budget and keep order, notes carry the part before, long notes are combined, a declined passage is split and counted, a whole chapter declined is an error, a smaller window than expected splits parts, cancelling saves nothing, summaries survive a relaunch and are replaced by Summarise Again | automated | Done | `cargo test summary` and `a_chapter_summary_is_saved_with_the_book`, `summaries_need_an_available_model_and_an_open_book` (fake model) |
| S2 | Summarise a long chapter of a real book on the iPhone (Apple Intelligence on): the sheet counts the parts, the summary streams in, it names the right people and events, and it is still there after closing and reopening the reader | physical device | **Not yet done** | |
| 10 | Change Rust logic, rebuild, reinstall, see the change on the phone | physical device | **Not yet done** | Rebuild detection has a regression test (`scripts/test-native-rebuild.sh`) |

## Framework problems found and fixed

| Problem | Layer | Fix | Regression test |
|---------|-------|-----|-----------------|
| On Linux every `forge build/run ios` rebuilt the Rust library: the build recorded the SDK path it resolved from xtool, but the next run's staleness check only sees the caller's `SDKROOT`, so they never matched | build scripts | `build-rust-ios.sh` records the SDKROOT it was given | `scripts/test-native-rebuild.sh` (stub cargo/rustup/swift; fails without the fix) |
| Leaving a catalogue search let its Rust request run to the 15 s timeout | app (consumes #30) | One `Cancellation` per catalogue search and import, triggered by Swift task cancellation; `shutdown` stops requests in flight | `a_cancelled_search_stops_at_once_and_changes_nothing`, `a_cancelled_import_adds_nothing`, `shutdown_stops_a_request_in_flight` (Rust); `testCancellingTheTaskStopsTheRustSearch` (XCTest) |
| Apps made with `forge init` inside the checkout have their own Cargo workspace, so CI's `cargo fmt/clippy/test --workspace` and the binding drift check never looked at them | CI / scripts | `scripts/check-app-crates.sh` (fmt, clippy, tests, Swift binding drift for every `examples/*/rust`), in CI, `forge check` and `make check-app-crates` | The script itself; checked to fail on a stale binding |

Still open:

- **App icon.** Forge can now declare one (`[ios] icon` in `forge.toml`, from #31), but Pocket Library has no icon artwork yet, so it shows the default.

## Manual acceptance on an iPhone (Linux)

The steps (first launch, adding by hand and from the catalogue, EPUB import, progress, edit and delete, search, persistence, offline, settings and export, the release build, and a Rust change reaching the phone) are in [docs/integration/iphone-acceptance.md](../../docs/integration/iphone-acceptance.md), sections E and F.
