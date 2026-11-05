# Compatibility matrix

Versions this repository is built and tested with. If something does not build, check your versions against this table first.

- **Pinned** means the repo enforces it (a lock file, a wrapper, `rust-toolchain.toml`, or an exact version in a build file).
- **CI** means a GitHub Actions job exercises it on every pull request.
- **Installed only** means the tool was installed and its basic commands run, but the full device flow has not been verified with it.

Last updated: 2026-11-05. Update this page in the same PR as any version bump.

## Rust and FFI

| Component | Tested version | Status | Where it is set |
|-----------|----------------|--------|-----------------|
| Rust toolchain | 1.97.0 | Pinned, CI | [`rust-toolchain.toml`](../rust-toolchain.toml) |
| Rust MSRV | 1.88 | CI (`cargo check` on 1.88.0) | `rust-version` in [`Cargo.toml`](../Cargo.toml) |
| UniFFI | 0.32.2 | Pinned, CI | `[workspace.dependencies]` in [`Cargo.toml`](../Cargo.toml) |
| Rust iOS target | `aarch64-apple-ios` (device only) | CI (macOS) | `rustup target add` |
| Rust Android targets | `aarch64-linux-android`, `x86_64-linux-android` | CI | `rustup target add`, ABIs in `forge.toml` |
| cargo-ndk | latest release | CI (not pinned) | `cargo install cargo-ndk` |

## iOS

| Component | Tested version | Status | Notes |
|-----------|----------------|--------|-------|
| Swift (Linux) | 6.2.3 (ubuntu24.04 tarball) | Manual device run | `swift-tools-version: 6.0` in `Package.swift`; xtool upstream recommends a newer Swift |
| xtool | 1.24.0 AppImage | Manual device run | Chat installed and run on an iPhone from Ubuntu 24.04 on 2026-11-05 with `xtool dev run -c release`; Counter does not link with this combination, see [troubleshooting.md](troubleshooting.md) |
| Xcode.xip (for `xtool setup`) | 16.0 | Manual device run | Provides the iOS 18.0 SDK |
| Xcode (macOS) | `macos-15` runner default (16.x) | CI | The CI log prints the exact version (`xcodebuild -version` step); docs require 16.0+ |
| iOS deployment target | 17.0 | Pinned | `minimum_ios` in `forge.toml`, Xcode project |
| macOS host for CI | macOS 15 | CI | `.github/workflows/macos.yml` |

Linux iOS builds have the least coverage: CI cannot run xtool (no Darwin SDK in hosted runners), so Linux iOS results come from manual runs recorded in [toolchain-validation.md](toolchain-validation.md).

## Android

| Component | Tested version | Status | Where it is set |
|-----------|----------------|--------|-----------------|
| Android Gradle Plugin | 8.7.3 | Pinned, CI | root `build.gradle.kts` (example and template) |
| Gradle | 8.10.2 | Pinned, CI | `gradle/wrapper/gradle-wrapper.properties` |
| Kotlin (+ Compose compiler plugin) | 2.0.21 | Pinned, CI | root `build.gradle.kts` |
| JDK | 17 (Temurin) | CI | `jvmTarget = "17"` in `app/build.gradle.kts` |
| Android NDK | r28 (`28.0.13004108`) | CI | `.github/workflows/android.yml`; r28+ needed for 16 KB page-size defaults |
| compileSdk / targetSdk | 35 | Pinned, CI | `app/build.gradle.kts` |
| minSdk | 26 | Pinned | `min_sdk` in `forge.toml` |
| Build tools | 35.0.0 | CI | `.github/workflows/android.yml` |
| JNA | 5.19.1 (`@aar` on device, plain jar for JVM tests) | Pinned, CI | `app/build.gradle.kts` |
| kotlinx-coroutines-core | 1.8.1 | Pinned, CI | `app/build.gradle.kts`; required by UniFFI async exports |

## Known incompatible

| Combination | Problem | Status |
|-------------|---------|--------|
| AGP 9.x / Gradle 9.x | Broke the Android CI build; rolled back in the example and template | Migration planned, see [roadmap.md](roadmap.md#agp--gradle-stabilization--planned) |
| `cargo-platform` > 0.3.2 | Raises its MSRV above 1.88 | Held at 0.3.2 in `Cargo.lock` |
| Windows hosts | Scripts are bash; not tested | Not supported |

## Reporting a version that works (or does not)

Open an issue with the output of `./scripts/validate-toolchain.sh` (or `forge doctor`), the commands you ran, and the result. A version moves from "Installed only" or "Not verified" to tested once the manual checklist in [toolchain-validation.md](toolchain-validation.md#manual-acceptance-counter-on-device) passes with it.
