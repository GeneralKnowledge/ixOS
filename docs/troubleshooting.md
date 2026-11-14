# Troubleshooting

Start with `forge doctor` when something won't build or install, and `forge diagnose` (in the app's folder) when an installed app doesn't work. `forge doctor` lists each missing tool, SDK, sign-in or device with the command that fixes it. `forge build ios` and `forge run ios` run the quick checks again before building (set `FORGE_SKIP_PREFLIGHT=1` to skip them), and when a build or install fails they end with a "What went wrong" section that names the layer (Rust library, Swift build, signing and provisioning, or iPhone) and a fix. The sections below explain those fixes.

## `SDKROOT is not set` when building Rust for iOS

Install the Darwin SDK:

```bash
xtool setup
# provide Xcode.xip path when prompted
swift sdk list   # should list darwin
./scripts/build-rust-ios.sh
```

## `rustup could not choose a version of cargo to run` in an app folder

rustup picks the toolchain from a `rust-toolchain.toml` in the current folder or above it, not from `--manifest-path`. Apps created before 2026-11-10 have the file only in `rust/`, so `cargo test --manifest-path rust/Cargo.toml` run from the app folder fails on a machine with no rustup default. Copy it up once: `cp rust/rust-toolchain.toml .`. Running cargo from inside `rust/` also works, and so does `forge test`. New apps have the file in both places.

## `swift: command not found`

Install Swift for your distro — see [linux-setup.md](linux-setup.md).

## UniFFI bindings out of date

```bash
./scripts/generate-bindings.sh
./scripts/check-bindings.sh
```

## `undefined symbol: swift_coroFrameAlloc` from `xtool dev build`

`forge build ios` and `forge run ios` catch this before building: when `swift --version` is 6.2 or later and xtool's Darwin SDK is older than iOS 26, they stop with this fix.

Swift 6.2 emits calls to this runtime function, and the iOS 18 SDK from Xcode 16 does not export it (`libswiftCore.tbd` has no such symbol), so the compiler and SDK versions do not match. Run `xtool setup` again with `Xcode.xip` from Xcode 26, whose iOS 26.0 SDK exports it.

Results with Swift 6.2.3 and xtool 1.24.0 on Linux:

| App | Xcode 16.0 (iOS 18.0 SDK), debug | Xcode 16.0, release | Xcode 26 (iOS 26.0 SDK), debug | Xcode 26, release |
|-----|------|------|------|------|
| Counter | Fails | Fails | Links | Links |
| Weather | Fails | Links | Links | Links |
| Chat | Fails | Links | Links | Links |
| App from `forge init` (old Counter template) | Not tested | Fails | Links | Links |
| App from `forge init` (own Rust crate, since 2026-11-06): Pocket Library | Not tested | Not tested | Links | Not tested |

On 2026-11-06, Chat built this way (Xcode 26, debug) was installed on an iPhone with `forge run ios` and opened there. A message posted to another room from the laptop showed a notification banner. Counter and Weather were installed and opened the same way. Pocket Library, which began as a `forge init` app with its own Rust crate, was built this way on 2026-11-07 and runs on the iPhone. A freshly generated app from today's template has not yet been recorded installing on a phone; CI builds it with Xcode on macOS.

## `... .pcm is not a valid precompiled module file` or `could not build Objective-C module 'AVFoundation'`

An earlier build was killed part way through (a dropped Remote Control or SSH session, Ctrl-C) and left truncated files in clang's module cache. The code is fine. In the app folder, delete the cache and build again:

```bash
rm -rf .build/arm64-apple-ios/*/ModuleCache
```

Forge recognises this failure and prints the same fix.

## `forge run ios` says usbmuxd is not running, or xtool seems to hang

xtool talks to the iPhone through usbmuxd. When usbmuxd is not running, xtool waits for a device forever without printing anything. On Linux, `forge run ios` and `forge doctor` check for usbmuxd's socket (`/var/run/usbmuxd`) first. On many distributions udev starts usbmuxd when an iPhone is plugged in, so plug it in and unlock it, or run `sudo systemctl start usbmuxd`.

If the phone is plugged in and unlocked but not listed, usbmuxd has usually lost it: `sudo systemctl restart usbmuxd`. Forge does this itself when the machine allows it ([linux-setup.md](linux-setup.md#let-forge-restart-usbmuxd-optional)).

## `forge run ios` finished but the app did not open

That is expected: `xtool dev run` installs the app but does not launch it, and `xtool launch` does not work on iOS 17 and later (upstream xtool issue #44). Tap the app's icon on the home screen. The first time, trust your developer profile first (Settings > General > VPN & Device Management).

## The app opens to a blank screen, or closes straight away

The build and install worked, so the problem is in the running app. With the iPhone still connected, run this in the app's folder:

```bash
forge diagnose
```

It checks, in order:
- whether the last `forge run ios` finished;
- that the iPhone is connected with Developer Mode on;
- that the app is installed and its signing hasn't expired (free accounts get 7 days);
- the newest crash report for the app, symbolicated;
- what the app logs while you open it (it asks you to, and listens for 15 seconds; `--for 1m` listens longer).

It ends with one **Next:** step, and exits non-zero while something is wrong. `--json` gives the same checks for scripts and agents.

To look yourself, the same information comes from these commands:

```bash
forge device logs --process <App>Xtool --for 30s   # then open the app; Rust and Swift log here
forge device crashes                               # did it crash? `--pull <dir>` saves and symbolicates the report
forge crash <dir>/*.ips                            # the crashed thread, with Swift and Rust file and line
```

The process name is the app's name followed by `Xtool` (`MyAppXtool`); `forge device apps` lists what is installed. A screen that stays empty usually means the root view is still waiting for Rust: look for a thrown error in the log, and check that `ContentView.swift` shows something while its model loads. Once `forge ui setup` has run, `forge ui tree` and `forge ui screenshot` show what is on screen ([forge-ui-testing.md](forge-ui-testing.md)). More in [forge-device.md](forge-device.md).

## `xtool devices` hangs or says `Resource temporarily unavailable`

Ensure usbmuxd is running and the device is trusted. `Resource temporarily unavailable` has been seen when usbmuxd stopped accepting connections; `sudo systemctl restart usbmuxd` and replugging the iPhone cleared it. Forge’s `validate-toolchain.sh` uses `timeout 5` on this command.

## SwiftPM link errors for `forge_ffi`

Ensure the static library exists:

```bash
ls build/ios-device/lib/libforge_ffi.a
./scripts/package-ios-library.sh examples/counter/Generated
```

Packaging scripts call `scripts/native-lib-needs-rebuild.sh` so changing Rust sources or iOS/Android build settings triggers a rebuild instead of reusing a stale `.a` / `.so`. Delete `build/ios-device/lib/libforge_ffi.a` (and its `.forge-build-meta` sidecar) to force a full iOS relink.

## `forge ui` fails with code 41, "Not authorized for performing UI testing actions"

The phone doesn't allow UI automation yet. Turn on **Settings > Developer > Enable UI Automation** on the iPhone (the Developer menu appears once Developer Mode is on), then run the command again. It is a one-time setting per phone; without it WebDriverAgent can't launch apps or touch anything ([forge-ui-testing.md](forge-ui-testing.md#running-m0)).

## Signing and iPhone problems

These come from the last step of `forge run ios`, when xtool signs the app and installs it. The error names the problem; here is what each one means.

| Message contains | Problem | Fix |
|------------------|---------|-----|
| `requires logging in with xtool auth`, `Logged out` | xtool has no Apple sign-in | `xtool auth login`, check with `xtool auth status` |
| `maximum number of apps for free development profiles` | A free account can have 3 development apps on a device at once | Remove another development app from the iPhone |
| `App ID limit` | Free accounts: 10 new bundle ids per 7 days | Reuse a bundle id (`bundle_id` in forge.toml), or wait |
| `provisioning profile`, `certificate has been revoked`, `maximum number of certificates` | Apple refused the certificate or profile | `xtool auth logout`, `xtool auth login`; revoke old development certificates at developer.apple.com if at the limit |
| `Developer Mode` | Developer Mode is off (iOS 16+) | Settings > Privacy & Security > Developer Mode, restart |
| `has not been explicitly trusted`, `invalid code signature` | Installed, but the profile is not trusted | Settings > General > VPN & Device Management > your Apple ID > Trust |
| `device is locked`, `PasswordProtected` | The iPhone is locked | Unlock it and keep it unlocked during the install |
| `InvalidHostID`, `could not connect to lockdownd`, `not paired` | Phone and computer are not paired | Replug, unlock, tap Trust; or `idevicepair pair` |
| `usbmuxd`, `Resource temporarily unavailable` | usbmuxd is stopped or stuck | `sudo systemctl restart usbmuxd`, replug |

If an app that used to open now closes at launch, its free-account signature has expired (after 7 days): run `forge run ios` again.

## Your Rust does not compile during `forge build ios`

The iOS build compiles your crate for `aarch64-apple-ios`, so a Rust error shows up there first. Fix it with the much faster host check, then build again:

```bash
cargo check --manifest-path rust/Cargo.toml
cargo test --manifest-path rust/Cargo.toml
```

## `undefined symbol: uniffi_...` when linking the app

The Swift bindings and the Rust static library come from different versions of your crate. Delete the app's `build/ios-device` folder and run `forge build ios` (or `forge run ios`) again; it rebuilds both together.

## Xcode on macOS

See [ios-xcode.md](ios-xcode.md). Use `forge build ios --package-only` before opening `examples/<app>/Xcode/<App>.xcodeproj`. Set a Development Team in Xcode; simulator builds are not supported until simulator Rust slices exist.

If SwiftPM rejects `-L` flags, see [docs/adr/001-ios-static-linking.md](adr/001-ios-static-linking.md) for the artifact-bundle fallback.

## Decrement shows error at zero

Expected — Rust returns `ForgeAppError.WouldUnderflow`; the UI should show a message without crashing.

## Persistence shows “Could not save counter state”

Rust returned `ForgeAppError.PersistenceFailed` after a failed JSON write (permissions or disk). **In-memory state is not committed** on a failed save—the counter stays at the last successfully persisted value. Refresh the UI from `snapshot()` if needed, then check Application Support directory permissions.

## Did the laptop run start?

A `forge run ios` started from somewhere else (Remote Control, an agent, a script over SSH) may be refused before it runs, and nothing on the phone says so. Every run writes `<app>/.forge/status/run-ios.json` the moment it starts: its `phase` (`starting`, `building`, `waiting_for_device`, `installing`, then `done` or `failed` with the `error`), the `pid`, and `updated`, which moves every 10 seconds while it runs. Read it within a minute of asking. If there is no file newer than the request, nothing started. If `updated` is more than a minute old and the phase isn't `done` or `failed`, the process died.

## Build without the phone, install when it's there

`forge run ios` never waits on the iPhone to compile. It checks the setup that would stop a build (xtool, the Darwin SDK, the Rust target, the Apple sign-in), builds the Rust and the Swift app, and only then looks for the phone (restarting a stuck usbmuxd once). With no phone it stops with `Built …, ready to install` (exit code 13, stage `device`, `data.built: true`). Running the same command again once the phone is connected rebuilds only what changed, which takes seconds, and then signs and installs.

So start `forge run ios` as soon as there is something to put on the phone, and ask for the phone only when it says the build is ready. With `--wait-for-device 20` it waits up to 20 minutes for the iPhone after the build instead of stopping, and its status says `waiting_for_device` at that point.

Signing happens with the install: xtool makes the provisioning profile for the connected phone, so it can't be done ahead. It takes seconds, not a rebuild.
