# Roadmap

Forge Mobile is an **integration reference**: a working Counter app on iOS and Android, a CLI, templates, and the docs that explain each piece. It is actively maintained. This page says what comes next, in rough priority order, so you can tell what is planned and where help is welcome.

Status labels: **In progress** (someone is on it), **Planned** (agreed, not started), **Open** (direction agreed, contributions welcome), **Not planned** (out of scope for now, with the reason).

Last reviewed: 2026-11-06.

## Near term

### Developer CLI polish — Done, follow-ups open

Done: `forge init --path`, with each app owning its Rust crate ([app-crate.md](app-crate.md)); `forge check` (the local CI matrix); `forge doctor`, which reports what each workflow needs with a fix for each missing piece, and `--json` for CI; `forge bindings generate --project`; and `--project` (for build, run and bindings) defaulting to the current app directory.

Open: `forge doctor` does not check device pairing, signing or SDK licences yet.

### iOS app configuration — Done, follow-ups open

Done: `forge.toml` is the one place for an iOS app's display name, version and build number, deployment target, icon, orientations, permission prompts, URL schemes, background modes, entitlements and signing team type. Forge generates Info.plist, App.entitlements and xtool.yml from it (and keeps Package.swift's `.iOS(...)` in step) on every build, `forge config check` guards it in CI, and `forge build/run ios --release` builds the release configuration ([app-config.md](app-config.md)).

Open: asset-catalog icons and launch screens (xtool cannot compile asset catalogs on Linux), per-configuration overrides (for example a separate debug bundle ID), app extensions (xtool supports them; Forge has no template), and applying `team_id` / `minimum_ios` to the Xcode project instead of reporting the difference.

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
- **Isolate xtool.** The iOS build steps sit behind an `IosBuilder` trait in `forge-cli` (done), so an xtool break, or a second backend (plain `xcodebuild` on macOS), touches one module instead of the whole CLI.
- **macOS stays a supported path.** CI already builds the Xcode projects on `macos-15`, so a Mac with Xcode is the fallback when an xtool release regresses.

## Medium term

### Networking — In progress

`forge-net` has a transport trait, test doubles, and a real blocking HTTPS client (`UreqClient`: ureq + rustls with the platform certificate verifier) behind its `ureq` feature. The iOS Weather example ([examples/weather](../examples/weather/README.md)) uses it end to end: Open-Meteo requests from Rust, typed errors, cancellation, and an offline cache. The Chat demo ([docs/chat-demo.md](chat-demo.md)) uses it on **both iOS and Android**: a long-lived Server-Sent Events stream per room, reconnects with catch-up, and idempotent retries, against a local mock server.

HTTPS works on Android too: hosts call `ForgeAndroid.init(applicationContext)` once, and certificates are checked against Android's trust store, with typed `Certificate` errors ([android-setup.md](android-setup.md#https-certificate-verification)). It is covered by Rust tests against local TLS servers and the JVM test of the JNI entry point; a run on a physical Android phone is still to be recorded.

Ordinary requests can be interrupted in flight: `HttpClient::send_cancellable` closes the connection, and the starter app crate wires it to Swift task cancellation and Kotlin coroutine cancellation ([app-crate.md](app-crate.md#lifecycle)). The Weather example still discards a late response instead of interrupting it. Still open: platform reachability.

### iOS Simulator support — Open

Builds are **device-only** today. The Rust library ships as a single `aarch64-apple-ios` static library ([ADR 001](adr/001-ios-static-linking.md)), and the iOS Simulator only runs on macOS, so in the Linux-first workflow a physical device is the only way to run the app anyway. Simulator support mainly helps Mac users and macOS CI. It needs:

1. An `aarch64-apple-ios-sim` (and optionally `x86_64-apple-ios`) slice.
2. Packaging that can hold more than one slice: an XCFramework or a SwiftPM artifact bundle (ADR 001's fallback).
3. `ios_simulator = true` in `forge.toml` wired through the CLI (config parsing exists; the build path does not).
4. A macOS CI job that runs the XCTest suite on a simulator.

Good first project for someone with a Mac.

### Store distribution (App Store, Google Play) — Open

Not in this release. Today the CLI produces development builds (an xtool debug build signed for your own device on iOS, a debug APK for Android; see [builds.md](builds.md)). Release distribution needs:

- **Android:** release build type, signing config read from the environment (never committed), an `.aab` bundle, and R8/minification rules that keep JNA and the generated bindings.
- **iOS:** an archive and export path. App Store submission needs a distribution certificate and provisioning profile; whether xtool can produce a submission-ready build on Linux has not been verified, so the first deliverable is a documented macOS `xcodebuild archive` path.
- A `docs/distribution.md` covering both, plus release notes in [release-process.md](release-process.md).

## Not planned (for now)

- **Windows development hosts.** Tooling targets Linux and macOS.
- **A generic event bus or callback interfaces across the FFI boundary.** Events are pulled through bounded subscriptions; see [ffi-policy.md](ffi-policy.md#concurrency-contract) for why.
- **Publishing crates to crates.io.** The workspace is source-only until the APIs settle.

## How to pick something up

Open an issue saying which item you want to work on and how you plan to approach it, then follow [CONTRIBUTING.md](../CONTRIBUTING.md). For anything touching the FFI boundary, read [ffi-policy.md](ffi-policy.md) first; for packaging or architecture decisions, add an ADR under [adr/](adr/).
