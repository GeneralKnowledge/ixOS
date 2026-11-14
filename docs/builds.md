# What each build produces

Forge has several ways to build an app. They differ in the host machine, the tool, where the app runs, and whether it is a debug or release build. This page lists them all, so it is clear which ones are supported and tested and which are not.

## At a glance

| Command or tool | Host | Platform tool | Runs on | Swift / Kotlin config | Rust profile | Status |
|---|---|---|---|---|---|---|
| `forge build ios` | Linux (or macOS) | xtool (`xtool dev build`) | iPhone (device) | debug | release | Supported. Counter, Weather and Chat installed on an iPhone (2026-11-06) |
| `forge run ios` | Linux (or macOS) | xtool (`xtool dev build`, then `xtool install` of that build once an iPhone is connected) | iPhone, installed (open it from the home screen; xtool does not launch apps) | debug | release | Supported, same runs as above |
| `forge build ios --release`, `forge run ios --release` | Linux (or macOS) | xtool (`xtool dev build -c release`) | iPhone (device) | release | release | Supported. Counter, Weather and Chat built in release with xtool on Linux (2026-11-06); not yet recorded on a device through this flag |
| `forge build ios --package-only`, then Xcode | macOS | Xcode (`xcodebuild`) | iPhone (device) | Debug or Release, chosen in Xcode | release | Supported. CI does unsigned Debug builds of the Chat, Pocket Library and freshly generated Xcode projects |
| `forge check swift` in an iOS app | Linux (or macOS) | Cargo, then plain SwiftPM (`swift test`), no iOS SDK | the host | debug | debug | Supported. Compiles the Swift files that import nothing Apple-only (view model, bindings) and runs the tests that need no SwiftUI; views are left out |
| `forge test` in an iOS app | Linux or macOS | Cargo, then SwiftPM (`swift test`) on macOS | the host | debug | debug | Supported, CI. The app's Rust tests run everywhere; the XCTest suite only on macOS (skipped with a note on Linux) |
| `forge build android` | Linux or macOS | Gradle (`assembleDebug`) | Android phone or emulator | debug APK | release | Supported. CI builds the APK; not yet run on a physical phone |
| `forge build android --install` | Linux or macOS | Gradle (`installDebug`) | connected phone or emulator | debug APK | release | Supported, not yet recorded on a physical phone |
| `forge test` in an Android app | Linux or macOS | Cargo, then Gradle (`testDebugUnitTest`) | the JVM (host tests) | debug | debug | Supported, CI |
| iOS Simulator | macOS | Xcode | Simulator | — | — | **Not supported**: no simulator slice of the Rust library ([roadmap](roadmap.md#ios-simulator-support--open)) |
| Release / store builds (App Store, Google Play) | — | — | — | — | — | **Not supported** yet. The staged plan for the first App Store release, and which steps need a Mac, is [first-app-store-release.md](first-app-store-release.md) |

Notes:

- **Device builds only on iOS.** The Rust library is built for `aarch64-apple-ios` alone ([ADR 001](adr/001-ios-static-linking.md)). A Linux machine cannot run the iOS Simulator anyway, so on Linux a physical iPhone is the only way to run an app.
- **The Rust library is optimised even in debug builds.** The build scripts default to Cargo's `release` profile for the native library, because a debug Rust build is large and slow on a phone. Set `FORGE_IOS_PROFILE=debug` or `FORGE_ANDROID_PROFILE=debug` to build it unoptimised. Host tests (`forge test`, `cargo test`) always use the debug profile.
- **xtool builds use a debug Swift configuration** unless you pass `--release`, and are signed with your Apple ID for your own devices ([ios-signing.md](ios-signing.md)). They are development builds, not App Store builds. With Swift 6.2 they need the Darwin SDK from Xcode 26 ([troubleshooting.md](troubleshooting.md)).
- **Android builds are debug APKs**, signed with the SDK's debug key. A release build type, signing from the environment, an `.aab` bundle and R8 rules are on the roadmap.
- **Emulators.** The Android ABIs in `forge.toml` include `x86_64`, so the APK also runs on an x86_64 emulator. Android HTTPS has only been tested on the JVM and in Rust tests so far ([toolchain-validation.md](toolchain-validation.md#manual-acceptance-android-https-on-a-device)).

## One build at a time

A machine that several people or sessions build on (the laptop the apps are installed from) can't take several iOS builds at once: Cargo, SwiftPM and xtool each use every core, and together they starve everything else, including a Remote Control session and the over-the-air install page. So the heavy part of `forge build ios`, `forge run ios`, `forge publish ios` and [`scripts/check-ios-xtool.sh`](ios-build-check.md) takes a machine-wide lock first. A second build says who holds it and waits its turn:

```text
[forge] Another build is running on this machine, so this one waits its turn (up to 60 min, FORGE_BUILD_LOCK_TIMEOUT). Running now: pid 4242 since 2026-11-14T15-10-02: forge run ios in /home/red/ixOS/examples/device-lab (branch claude/ota-fix) [PR #189]
```

- `forge run ios` holds the lock only while it builds; waiting for the iPhone and installing don't hold up the next build. Its status file shows `waiting_for_build_lock` meanwhile.
- The lock is `flock` on `~/.cache/forge/build.lock` (or `$XDG_CACHE_HOME/forge/build.lock`). The kernel frees it when the holder exits, however it exits, so there are no stale locks to clear. A build that hangs keeps it: the waiter gives up after an hour and names the holder's pid, so you can stop that process.
- Builds run at nice 10 and the lowest best-effort I/O priority, so interactive work on the same machine comes first.

| Variable | Effect |
|---|---|
| `FORGE_BUILD_LOCK_NOTE` | Who is building (a thread, PR or person), shown to whoever waits |
| `FORGE_BUILD_LOCK_TIMEOUT` | Seconds to wait for the lock (default 3600) |
| `FORGE_BUILD_LOCK` | Another lock file, or `off` for no lock (a machine only you build on) |
| `FORGE_BUILD_NICE=0` | Keep normal CPU and disk priority |

## Which one should I use?

- **On Linux with an iPhone:** use `forge run ios` from the app directory. Run `forge doctor` first: it lists anything missing for "iOS from Linux" and "Run on an iPhone" along with how to fix it. `forge run ios` repeats the quick checks before it builds.
- **On Linux with an Android phone:** use `forge build android --install`.
- **On a Mac:** both of the above work too. Use Xcode when you want the debugger or Instruments ([ios-xcode.md](ios-xcode.md)).
- **Changing the app's name, version, permissions or entitlements:** edit `forge.toml`; the next build writes Info.plist, App.entitlements and xtool.yml ([app-config.md](app-config.md)).
- **No device:** use `forge test` (the app's Rust tests, plus JVM tests on any host or XCTest on macOS). `forge test --workspace` also runs Forge's own tests.

Tested tool versions are in [compatibility.md](compatibility.md).
