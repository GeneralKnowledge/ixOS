# Roadmap

Forge Mobile is an **integration reference**: a working Counter app on iOS and Android, a CLI, templates, and the docs that explain each piece. It is actively maintained. This page says what comes next, in rough priority order, so you can tell what is planned and where help is welcome.

Status labels: **In progress** (someone is on it), **Planned** (agreed, not started), **Open** (direction agreed, contributions welcome), **Not planned** (out of scope for now, with the reason).

Last reviewed: 2026-11-04.

## Near term

### Developer CLI polish — In progress

- `forge init --path <dir>` so apps can be created outside the workspace's `examples/` (today they always land in `examples/<Name>`; see [project-generation.md](project-generation.md)).
- `forge check`: one command that runs the local CI matrix (workspace tests, rustfmt, binding drift, Android JVM unit tests where a JDK and SDK are present). Until it lands, `make validate` plus `make test-android-host` covers the same ground.
- `forge doctor --json`: machine-readable toolchain report for CI and scripts.

### AGP / Gradle stabilization — Planned

Android builds are pinned to **AGP 8.7.3 + Gradle 8.10.2** (see [compatibility.md](compatibility.md)). A Dependabot bump to AGP 9 / Gradle 9 broke the Android CI job and was rolled back, in the example and the template. The plan:

1. Migrate `examples/counter-android` and `templates/android-app` together, in one PR, with the Android CI job green.
2. Keep the example and template plugin versions identical (they are the same contract).
3. Update the compatibility matrix in the same PR.

Until then, Dependabot Gradle PRs that cross a major AGP or Gradle version should be closed, not merged.

### xtool / Xcode compatibility plan — Planned

Linux iOS builds depend on [xtool](https://github.com/xtool-org/xtool) and the Darwin SDK it extracts from an `Xcode.xip`. Both move independently of this repo, so:

- **Pin what is tested.** [compatibility.md](compatibility.md) lists the xtool, Swift and Xcode versions that have been exercised, and which ones have only been installed but not run end to end.
- **Record real runs.** A version is promoted to "verified" only after the manual device checklist in [toolchain-validation.md](toolchain-validation.md) passes with it.
- **Isolate xtool.** The iOS build steps are being moved behind an `IosBuilder` trait in `forge-cli` (**In progress**), so an xtool break, or a second backend (plain `xcodebuild` on macOS), touches one module instead of the whole CLI.
- **macOS stays a supported path.** CI already builds the Xcode projects on `macos-15`, so a Mac with Xcode is the fallback when an xtool release regresses.

## Medium term

### Networking — In progress

`forge-net` has a transport trait, test doubles, and a real blocking HTTPS client (`UreqClient`: ureq + rustls with the platform certificate verifier) behind its `ureq` feature. The iOS Weather example ([examples/weather](../examples/weather/README.md)) uses it end to end: Open-Meteo requests from Rust, typed errors, cancellation, and an offline cache.

Still open: Android (the certificate verifier needs a JNI init call and its companion library), interrupting a request in flight (ureq cannot; cancellation currently discards the late response), and platform reachability.

### iOS Simulator support — Open

Builds are **device-only** today. The Rust library ships as a single `aarch64-apple-ios` static library ([ADR 001](adr/001-ios-static-linking.md)), and the iOS Simulator only runs on macOS, so in the Linux-first workflow a physical device is the only way to run the app anyway. Simulator support mainly helps Mac users and macOS CI. It needs:

1. An `aarch64-apple-ios-sim` (and optionally `x86_64-apple-ios`) slice.
2. Packaging that can hold more than one slice: an XCFramework or a SwiftPM artifact bundle (ADR 001's fallback).
3. `ios_simulator = true` in `forge.toml` wired through the CLI (config parsing exists; the build path does not).
4. A macOS CI job that runs the XCTest suite on a simulator.

Good first project for someone with a Mac.

### Store distribution (App Store, Google Play) — Open

Not in this release. Today the CLI produces development builds (an xtool release-configuration build signed for your device on iOS, a debug APK for Android). Release distribution needs:

- **Android:** release build type, signing config read from the environment (never committed), an `.aab` bundle, and R8/minification rules that keep JNA and the generated bindings.
- **iOS:** an archive and export path. App Store submission needs a distribution certificate and provisioning profile; whether xtool can produce a submission-ready build on Linux has not been verified, so the first deliverable is a documented macOS `xcodebuild archive` path.
- A `docs/distribution.md` covering both, plus release notes in [release-process.md](release-process.md).

## Not planned (for now)

- **Windows development hosts.** Tooling targets Linux and macOS.
- **A generic event bus or callback interfaces across the FFI boundary.** Events are pulled through bounded subscriptions; see [ffi-policy.md](ffi-policy.md#concurrency-contract) for why.
- **Publishing crates to crates.io.** The workspace is source-only until the APIs settle.

## How to pick something up

Open an issue saying which item you want to work on and how you plan to approach it, then follow [CONTRIBUTING.md](../CONTRIBUTING.md). For anything touching the FFI boundary, read [ffi-policy.md](ffi-policy.md) first; for packaging or architecture decisions, add an ADR under [adr/](adr/).
