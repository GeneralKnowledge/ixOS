# Troubleshooting

Start with `forge doctor`: it lists each missing tool, SDK, sign-in or device with the command that fixes it. `forge build ios` and `forge run ios` run the quick checks again before building (set `FORGE_SKIP_PREFLIGHT=1` to skip them), and when a build or install fails they end with a "What went wrong" section that names the layer (Rust library, Swift build, signing and provisioning, or iPhone) and a fix. The sections below explain those fixes.

## `SDKROOT is not set` when building Rust for iOS

Install the Darwin SDK:

```bash
xtool setup
# provide Xcode.xip path when prompted
swift sdk list   # should list darwin
./scripts/build-rust-ios.sh
```

## `swift: command not found`

Install Swift for your distro — see [linux-setup.md](linux-setup.md).

## UniFFI bindings out of date

```bash
./scripts/generate-bindings.sh
./scripts/check-bindings.sh
```

## `undefined symbol: swift_coroFrameAlloc` from `xtool dev build`

Swift 6.2 emits calls to this runtime function, and the iOS 18 SDK from Xcode 16 does not export it (`libswiftCore.tbd` has no such symbol), so the compiler and SDK versions do not match. Run `xtool setup` again with `Xcode.xip` from Xcode 26, whose iOS 26.0 SDK exports it.

Results with Swift 6.2.3 and xtool 1.24.0 on Linux:

| App | Xcode 16.0 (iOS 18.0 SDK), debug | Xcode 16.0, release | Xcode 26 (iOS 26.0 SDK), debug | Xcode 26, release |
|-----|------|------|------|------|
| Counter | Fails | Fails | Links | Links |
| Weather | Fails | Links | Links | Links |
| Chat | Fails | Links | Links | Links |
| App from `forge init` (old Counter template) | Not tested | Fails | Links | Links |
| App from `forge init` (own Rust crate, since 2026-11-06) | Not tested | Not tested | Not tested | Not tested |

On 2026-11-06, Chat built this way (Xcode 26, debug) was installed on an iPhone with `forge run ios` and opened there. A message posted to another room from the laptop showed a notification banner. Counter and Weather were installed and opened the same way. Apps from `forge init` have only been built, not installed, and the current template (each app with its own Rust crate) has not yet been built with xtool at all; CI builds it with Xcode on macOS.

## `xtool devices` hangs or says `Resource temporarily unavailable`

Ensure usbmuxd is running and the device is trusted. `Resource temporarily unavailable` has been seen when usbmuxd stopped accepting connections; `sudo systemctl restart usbmuxd` and replugging the iPhone cleared it. Forge’s `validate-toolchain.sh` uses `timeout 5` on this command.

## SwiftPM link errors for `forge_ffi`

Ensure the static library exists:

```bash
ls build/ios-device/lib/libforge_ffi.a
./scripts/package-ios-library.sh examples/counter/Generated
```

Packaging scripts call `scripts/native-lib-needs-rebuild.sh` so changing Rust sources or iOS/Android build settings triggers a rebuild instead of reusing a stale `.a` / `.so`. Delete `build/ios-device/lib/libforge_ffi.a` (and its `.forge-build-meta` sidecar) to force a full iOS relink.

## Signing and iPhone problems

These come from the last step of `forge run ios`, when xtool signs the app and installs it. The error names the problem; here is what each one means.

| Message contains | Problem | Fix |
|------------------|---------|-----|
| `requires logging in with xtool auth`, `Logged out` | xtool has no Apple sign-in | `xtool auth login`, check with `xtool auth status` |
| `maximum number of apps for free development profiles` | Free accounts: 3 sideloaded apps per device | Remove another development app from the iPhone |
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
