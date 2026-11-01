# Building iOS apps in Xcode (macOS)

Forge Mobile keeps **one Rust core** and **one SwiftPM package** per app. Linux developers use **xtool** against the SwiftPM executable product; macOS developers open a conventional **`.xcodeproj` application host** that links the same SwiftPM library products.

## Requirements

| Component | Minimum |
|-----------|---------|
| macOS | 14 (Sonoma) or newer recommended |
| Xcode | 16.0+ (Swift 6 toolchain, iOS 17 SDK) |
| Rust | Same as repo `rust-toolchain.toml` |
| Rust target | `aarch64-apple-ios` (`rustup target add aarch64-apple-ios`) |

Signing credentials and provisioning profiles stay **outside** the repository. Set **Development Team** in Xcode for the `Counter` (or generated app) target.

## Counter example

1. From the repository root on macOS, package Rust + bindings (uses Xcode SDK via `xcrun` when available):

   ```bash
   cargo run -p forge-cli -- build ios --project examples/counter --package-only
   ```

2. Open the application host:

   ```bash
   open examples/counter/Xcode/Counter.xcodeproj
   ```

3. Select a **physical iPhone** run destination (see simulator note below).

4. Pick the shared **App** scheme (not the `Counter` scheme Xcode generates for the Swift package's executable product), then build and run (⌘R). Increment/decrement, force an invalid decrement at zero, then terminate and relaunch to confirm persistence under Application Support.

The **Build Rust FFI** run script phase calls `scripts/package-ios-library.sh` so Xcode rebuilds the static library when Rust sources or build metadata change.

## Generated apps (`forge init`)

After `forge init MyApp --bundle-id org.example.myapp --platform ios`:

- SwiftPM executable + library: `examples/MyApp/Package.swift` (xtool on Linux)
- Xcode host: `examples/MyApp/Xcode/MyApp.xcodeproj`

Replace `MyApp` with your project name.

## Simulator vs device

v1 ships a **device-only** `aarch64-apple-ios` static library ([ADR 001](adr/001-ios-static-linking.md)). The Xcode target is configured for **`iphoneos` only**. Simulator builds are expected to fail until simulator slices or an XCFramework are added.

## Regenerating bindings

From the repo root:

```bash
cargo run -p forge-cli -- bindings generate --out examples/counter/Generated
```

Do not edit files under `Generated/`. Rebuild in Xcode afterward.

## Verification status

| Check | CI (Linux) | Manual (macOS + device) |
|-------|------------|-------------------------|
| `scripts/validate-xcode-project.sh` | Yes | — |
| Xcode build | No macOS runner in CI | Required for release confidence |
| Physical device install | No | Required for acceptance |
