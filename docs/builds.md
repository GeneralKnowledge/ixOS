# What each build produces

Forge has several ways to build an app. They differ in the host machine, the tool, where the app runs, and whether it is a debug or release build. This page lists them all, so it is clear which ones are supported and tested and which are not.

## At a glance

| Command or tool | Host | Platform tool | Runs on | Swift / Kotlin config | Rust profile | Status |
|---|---|---|---|---|---|---|
| `forge build ios` | Linux (or macOS) | xtool (`xtool dev build`) | iPhone (device) | debug | release | Supported. Counter, Weather and Chat installed on an iPhone (2026-11-06) |
| `forge run ios` | Linux (or macOS) | xtool (`xtool dev run`) | iPhone, installed and launched | debug | release | Supported, same runs as above |
| `forge build ios --release`, `forge run ios --release` | Linux (or macOS) | xtool (`xtool dev build -c release`) | iPhone (device) | release | release | Supported. Counter, Weather and Chat built in release with xtool on Linux (2026-11-06); not yet recorded on a device through this flag |
| `forge build ios --package-only`, then Xcode | macOS | Xcode (`xcodebuild`) | iPhone (device) | Debug or Release, chosen in Xcode | release | Supported. CI does unsigned Debug builds of every Xcode project |
| `forge test --project <ios app>` | macOS | SwiftPM (`swift test`) | the Mac (host tests) | debug | debug | Supported, CI |
| `forge build android` | Linux or macOS | Gradle (`assembleDebug`) | Android phone or emulator | debug APK | release | Supported. CI builds the APK; not yet run on a physical phone |
| `forge build android --install` | Linux or macOS | Gradle (`installDebug`) | connected phone or emulator | debug APK | release | Supported, not yet recorded on a physical phone |
| `forge test --project <android app>` | Linux or macOS | Gradle (`testDebugUnitTest`) | the JVM (host tests) | debug | debug | Supported, CI |
| iOS Simulator | macOS | Xcode | Simulator | — | — | **Not supported**: no simulator slice of the Rust library ([roadmap](roadmap.md#ios-simulator-support--open)) |
| Release / store builds (App Store, Google Play) | — | — | — | — | — | **Not supported** yet ([roadmap](roadmap.md#store-distribution-app-store-google-play--open)) |

Notes:

- **Device builds only on iOS.** The Rust library is built for `aarch64-apple-ios` alone ([ADR 001](adr/001-ios-static-linking.md)). A Linux machine cannot run the iOS Simulator anyway, so on Linux a physical iPhone is the only way to run an app.
- **The Rust library is optimised even in debug builds.** The build scripts default to Cargo's `release` profile for the native library, because a debug Rust build is large and slow on a phone. Set `FORGE_IOS_PROFILE=debug` or `FORGE_ANDROID_PROFILE=debug` to build it unoptimised. Host tests (`forge test`, `cargo test`) always use the debug profile.
- **xtool builds use a debug Swift configuration** unless you pass `--release`, and are signed with your Apple ID for your own devices ([ios-signing.md](ios-signing.md)). They are development builds, not App Store builds. With Swift 6.2 they need the Darwin SDK from Xcode 26 ([troubleshooting.md](troubleshooting.md)).
- **Android builds are debug APKs**, signed with the SDK's debug key. A release build type, signing from the environment, an `.aab` bundle and R8 rules are on the roadmap.
- **Emulators.** The Android ABIs in `forge.toml` include `x86_64`, so the APK also runs on an x86_64 emulator. Android HTTPS has only been tested on the JVM and in Rust tests so far ([toolchain-validation.md](toolchain-validation.md#manual-acceptance-android-https-on-a-device)).

## Which one should I use?

- **On Linux with an iPhone:** use `forge run ios --project .`. Run `forge doctor` first: it lists anything missing for "iOS from Linux" along with how to fix it.
- **On Linux with an Android phone:** use `forge build android --project . --install`.
- **On a Mac:** both of the above work too. Use Xcode when you want the debugger or Instruments ([ios-xcode.md](ios-xcode.md)).
- **Changing the app's name, version, permissions or entitlements:** edit `forge.toml`; the next build writes Info.plist, App.entitlements and xtool.yml ([app-config.md](app-config.md)).
- **No device:** use `forge test --project .` (JVM tests on any host, XCTest on macOS) and `cargo test --manifest-path rust/Cargo.toml`.

Tested tool versions are in [compatibility.md](compatibility.md).
