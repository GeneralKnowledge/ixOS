# Troubleshooting

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
| App from `forge init` | Not tested | Fails | Links | Links |

On 2026-11-06, Chat built this way (Xcode 26, debug) was installed on an iPhone with `forge run ios` and opened there. A message posted to another room from the laptop showed a notification banner. Counter, Weather and `forge init` apps have only been built, not installed.

## `xtool devices` hangs or says `Resource temporarily unavailable`

Ensure usbmuxd is running and the device is trusted. `Resource temporarily unavailable` has been seen when usbmuxd stopped accepting connections; `sudo systemctl restart usbmuxd` and replugging the iPhone cleared it. Forge’s `validate-toolchain.sh` uses `timeout 5` on this command.

## SwiftPM link errors for `forge_ffi`

Ensure the static library exists:

```bash
ls build/ios-device/lib/libforge_ffi.a
./scripts/package-ios-library.sh examples/counter/Generated
```

Packaging scripts call `scripts/native-lib-needs-rebuild.sh` so changing Rust sources or iOS/Android build settings triggers a rebuild instead of reusing a stale `.a` / `.so`. Delete `build/ios-device/lib/libforge_ffi.a` (and its `.forge-build-meta` sidecar) to force a full iOS relink.

## Xcode on macOS

See [ios-xcode.md](ios-xcode.md). Use `forge build ios --package-only` before opening `examples/<app>/Xcode/<App>.xcodeproj`. Set a Development Team in Xcode; simulator builds are not supported until simulator Rust slices exist.

If SwiftPM rejects `-L` flags, see [docs/adr/001-ios-static-linking.md](adr/001-ios-static-linking.md) for the artifact-bundle fallback.

## Decrement shows error at zero

Expected — Rust returns `ForgeAppError.WouldUnderflow`; the UI should show a message without crashing.

## Persistence shows “Could not save counter state”

Rust returned `ForgeAppError.PersistenceFailed` after a failed JSON write (permissions or disk). **In-memory state is not committed** on a failed save—the counter stays at the last successfully persisted value. Refresh the UI from `snapshot()` if needed, then check Application Support directory permissions.
