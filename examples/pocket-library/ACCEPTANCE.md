# Pocket Library acceptance report

What has been checked, where, and what is still to do. Results are kept apart by kind: **automated** (runs in CI on every PR), **local build** (run by hand on a development machine), and **physical device** (an iPhone). Nothing here was tested on the iOS Simulator, which Forge does not support.

Status as of 2026-11-07. A step is marked done only when it was actually performed.

## Toolchain

| Component | Version | Where it was used |
|-----------|---------|-------------------|
| Rust | 1.97.0 (`rust-toolchain.toml`) | Linux (cloud dev environment); CI |
| UniFFI / uniffi-bindgen | 0.32.2 | Linux; CI |
| xtool | *to record from the device run* (`xtool --version`) | — |
| Swift (Linux) | *to record* (`swift --version`) | — |
| Darwin SDK | *to record* (Xcode.xip used with `xtool setup`; Forge requires Xcode 26 / iOS 26.0 SDK) | — |
| Xcode (CI) | `macos-15` runner default, printed in the CI log | CI |

## The workflow

| # | Step | Kind | Status | Evidence |
|---|------|------|--------|----------|
| 1 | Generate the app: `forge init PocketLibrary --platform ios --bundle-id org.forgemobile.pocketlibrary --path examples/pocket-library` | local build (Linux) | Done | The starter was generated, then replaced screen by screen; `forge.toml`, `Package.swift`, `xtool.yml` and the Xcode project are the generated ones |
| 2 | Build and test its Rust crate on Linux | local build + automated | Done | 45 tests pass; clippy clean; `scripts/check-app-crates.sh` runs them in CI |
| 2b | Rust crate compiles for `aarch64-apple-ios` | local build (Linux) | Type-checked only | `cargo check --target aarch64-apple-ios` with stub C headers for `ring` (no Darwin SDK in the cloud environment). The real build happens in step 5 |
| 3 | Generate the Swift bindings | local build + automated | Done | `forge bindings generate --project`; CI fails if the committed `Generated/` drifts from the crate |
| 4 | Compile the SwiftUI host | automated (macOS CI) | Pending CI | XCTest suite (`test-swift-host.sh`) and an unsigned `xcodebuild` of the iOS app |
| 5 | Build the iOS app with xtool on Linux (debug and release) | physical device | **Not yet done** | Needs a Linux machine with xtool and the Darwin SDK |
| 6 | Install and launch on a physical iPhone | physical device | **Not yet done** | |
| 7 | Add, edit and delete books | physical device | **Not yet done** | Covered on the Mac host by XCTest (step 4) |
| 8 | Restart the app and check persistence | physical device | **Not yet done** | Covered in Rust (`everything_survives_a_relaunch`) and XCTest |
| 9 | Catalogue lookup and failure handling (airplane mode) | physical device | **Not yet done** | Parsing and every failure type are covered by Rust tests on canned responses; openlibrary.org is not reachable from the cloud environment, so no live request has been made yet |
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

Run on the Linux machine with xtool and the phone plugged in and unlocked. Record the results in the table above and the versions at the top.

```bash
git fetch origin && git checkout <this branch>
cd examples/pocket-library
xtool --version; swift --version; forge doctor        # record the versions
cargo test --manifest-path rust/Cargo.toml             # 45 passed
forge run ios                                          # debug build, installs and launches
```

On the phone:

1. **First launch.** The Library tab says "Your library is empty".
2. **Add by hand.** + → Enter Details: a title, author, 300 pages, current page 20. Save. The book opens; status is Reading (Rust started it because the page is past 0).
3. **Catalogue.** + → Search Online Catalogue → "the hobbit" → tap ⊕ on a result. It opens with a cover and description. Search again: the result shows a green check.
4. **EPUB.** Put an `.epub` in Files (AirDrop or iCloud Drive), then + → Import EPUB File. Title, author, cover and an estimated page count appear.
5. **Progress.** On a book, type a page and tap Save Page, or use the stepper. Type the last page: it becomes Finished with a haptic. Activity shows the session and the finish.
6. **Edit and delete.** Edit a book's title and rating. Swipe left on another in the Library → Delete → confirm.
7. **Search, filter, sort.** Search by part of an author's name; use the filter menu (three lines in a circle) to show one status and change the order.
8. **Persistence.** Swipe the app away in the app switcher, open it again: everything is as you left it.
9. **Offline.** Turn on airplane mode. Browsing, adding, editing and deleting still work. Catalogue search shows "You're offline" with Try Again and Enter Details by Hand; the library is unchanged.
10. **Settings.** Turn off online search: the + menu's catalogue item is disabled. Export Library → Share Export File → save to Files. (Leave Erase for last, or skip it.)
11. **Release build.** `forge run ios --release`; repeat 1, 5 and 8 briefly.
12. **Rust change reaches the phone.** In `rust/src/domain.rs`, change `SESSION_GAP_MS` to `5 * 1000` (five seconds). Run `forge run ios` and check the log says it rebuilt the Rust library. On the phone, update a book's page, wait ten seconds, update it again: Activity now shows two entries instead of one. Revert the change.
