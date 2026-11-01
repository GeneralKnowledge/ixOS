# Forge Mobile

Linux-first, Rust-first framework for building native mobile apps with **Rust** (domain logic), **UniFFI** (FFI), and thin platform hosts: **SwiftUI + xtool** (iOS) and **Jetpack Compose + Gradle** (Android).

This repository (`ixOS` on GitHub) ships a working **Counter** example, developer CLI, scripts, and documentation. It is an integration reference, not a full mobile platform yet.

The Rust-to-Swift bridge is asynchronous: SwiftUI talks to a thin Swift layer (`CounterBridge`) that exposes Rust commands and queries as `async` calls and Rust events as an `AsyncSequence`, with explicit cancellation and typed errors. See [docs/architecture.md](docs/architecture.md) and [docs/ffi-policy.md](docs/ffi-policy.md).

## Quick start (Linux)

1. Install [Rust](https://www.rust-lang.org/), [Swift](https://www.swift.org/), and [xtool](https://github.com/xtool-org/xtool) — see [docs/linux-setup.md](docs/linux-setup.md).
2. Run `xtool setup` with your **Xcode.xip** to install the Darwin SDK.
3. Validate: `./scripts/validate-toolchain.sh`
4. Test Rust: `cargo test --workspace`
5. Generate bindings: `./scripts/generate-bindings.sh`
6. Build Counter for iOS: `cargo run -p forge-cli -- build ios --project examples/counter`

Android (SDK + NDK on Linux) — see [docs/android-setup.md](docs/android-setup.md):

```bash
cargo run -p forge-cli -- build android --project examples/counter-android
```

CI assembles a **debug APK** on Ubuntu, runs Rust checks, binding drift tests, and validates the **Xcode project layout** for the Counter example. Full iOS device builds and Xcode compilation require macOS (see [docs/ios-xcode.md](docs/ios-xcode.md)); Linux iOS builds still use the Darwin SDK via xtool.

## Repository layout

```
crates/forge-core      # Domain logic (testable on Linux)
crates/forge-storage   # JSON snapshot persistence
crates/forge-ffi       # UniFFI boundary
crates/forge-cli       # `forge` CLI
examples/counter          # SwiftUI + xtool app
examples/counter-android  # Compose + Gradle app
templates/ios-app         # `forge init --platform ios`
templates/android-app     # `forge init --platform android`
scripts/               # Toolchain and build scripts
docs/                  # Architecture, setup, validation
```

## Forge CLI

```bash
cargo run -p forge-cli -- doctor
cargo run -p forge-cli -- init MyApp --bundle-id org.example.myapp --platform ios
cargo run -p forge-cli -- test   # workspace tests + rustfmt check
cargo run -p forge-cli -- test --project examples/counter-android  # ...plus that app's host tests
cargo run -p forge-cli -- bindings generate --out examples/counter/Generated
cargo run -p forge-cli -- bindings generate --language kotlin --out examples/counter-android/Generated
cargo run -p forge-cli -- build ios --project examples/counter       # xtool dev build
cargo run -p forge-cli -- run ios --project examples/counter         # build, install and launch (xtool dev)
cargo run -p forge-cli -- build ios --project examples/counter --package-only  # Rust + bindings only (Xcode on macOS)
cargo run -p forge-cli -- build android --project examples/counter-android
cargo run -p forge-cli -- devices
```

Install the binary: `cargo install --path crates/forge-cli --locked` (optional). The installed `forge` finds the workspace from the current directory (any folder inside the Forge checkout), or from `FORGE_WORKSPACE` if set. New apps are still created under the workspace's `examples/`.

## Configuration

Example apps include `forge.toml` — see [examples/counter/forge.toml](examples/counter/forge.toml).

New apps: `forge init MyApp --bundle-id org.example.myapp --platform ios|android` — see [docs/project-generation.md](docs/project-generation.md).

## License

Dual licensed under **MIT OR Apache-2.0** — see [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE).

## Known limitations

- Device-first; simulator off by default in config.
- iOS Rust linking requires xtool’s Darwin SDK (Xcode.xip).
- GitHub Actions CI runs Rust + binding checks, Android JVM unit tests and a **debug APK** build, and on macOS the Swift host tests plus an unsigned `xcodebuild` of the Counter and template Xcode projects.
- No App Store distribution or networking in this release.

See [docs/toolchain-validation.md](docs/toolchain-validation.md) for verified commands and blockers.
