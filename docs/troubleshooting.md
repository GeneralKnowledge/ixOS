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

## `xtool devices` hangs

Ensure usbmuxd is running and the device is trusted. Forge’s `validate-toolchain.sh` uses `timeout 5` on this command.

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
