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
./scripts/package-ios-library.sh
```

If SwiftPM rejects `-L` flags, see [docs/adr/001-ios-static-linking.md](adr/001-ios-static-linking.md) for the artifact-bundle fallback.

## Decrement shows error at zero

Expected — Rust returns `ForgeAppError.wouldUnderflow`; the UI should show a message without crashing.

## Persistence shows “Save issue”

Rust failed to write JSON (permissions or disk). State in memory may still update on failed save — check logs and directory permissions under Application Support.
