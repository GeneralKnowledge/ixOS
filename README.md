# Forge Mobile

[![Rust](https://github.com/GeneralKnowledge/ixOS/actions/workflows/rust.yml/badge.svg?branch=main)](https://github.com/GeneralKnowledge/ixOS/actions/workflows/rust.yml)
[![Bindings](https://github.com/GeneralKnowledge/ixOS/actions/workflows/bindings.yml/badge.svg?branch=main)](https://github.com/GeneralKnowledge/ixOS/actions/workflows/bindings.yml)
[![Android](https://github.com/GeneralKnowledge/ixOS/actions/workflows/android.yml/badge.svg?branch=main)](https://github.com/GeneralKnowledge/ixOS/actions/workflows/android.yml)
[![macOS](https://github.com/GeneralKnowledge/ixOS/actions/workflows/macos.yml/badge.svg?branch=main)](https://github.com/GeneralKnowledge/ixOS/actions/workflows/macos.yml)

Linux-first, Rust-first framework for building native mobile apps with **Rust** (domain logic), **UniFFI** (FFI), and thin platform hosts: **SwiftUI + xtool** (iOS) and **Jetpack Compose + Gradle** (Android).

This repository (`ixOS` on GitHub) ships a working **Counter** example, developer CLI, scripts, and documentation. It is an integration reference, not a full mobile platform yet; see [docs/roadmap.md](docs/roadmap.md) for what comes next and where help is welcome.

The Rust-to-Swift bridge is asynchronous: SwiftUI talks to a thin Swift layer (`CounterBridge`) that exposes Rust commands and queries as `async` calls and Rust events as an `AsyncSequence`, with explicit cancellation and typed errors. See [docs/architecture.md](docs/architecture.md) and [docs/ffi-policy.md](docs/ffi-policy.md).

## Why Forge?

Forge is for teams that want **native UI on each platform** (SwiftUI, Jetpack Compose) over **one Rust core**, and want to do most of that work **from Linux**. It is not a cross-platform UI toolkit: you write each app's UI in its own language, and Forge supplies the shared core, the FFI contract (async commands, events, cancellation, typed errors), the packaging scripts, a CLI and templates.

How it compares, briefly and honestly. **[Tauri](https://tauri.app/)** (webview UI) and **[Dioxus](https://dioxuslabs.com/)** (Rust UI) give you one UI for all platforms and are far more mature; **[Bevy](https://bevyengine.org/)** is a game engine with its own renderer. All three expect a Mac with Xcode for iOS builds today, while Forge builds and installs iOS apps from Linux through [xtool](https://github.com/xtool-org/xtool) (macOS with Xcode also works). **Plain [UniFFI](https://mozilla.github.io/uniffi-rs/)** is what Forge uses underneath; with UniFFI alone you write the iOS/Android packaging, binding regeneration, drift checks, host app wiring and the threading contract yourself. Pick Forge if native UI and a Linux-first workflow matter more to you than a shared UI or a large ecosystem; pick Tauri or Dioxus if you want one UI codebase; pick Bevy for games.

## Quick start (Linux)

1. Install [Rust](https://www.rust-lang.org/), [Swift](https://www.swift.org/), and [xtool](https://github.com/xtool-org/xtool) — see [docs/linux-setup.md](docs/linux-setup.md).
2. Run `xtool setup` with your **Xcode.xip** to install the Darwin SDK.
3. Validate: `./scripts/validate-toolchain.sh`. If anything fails, [docs/toolchain-validation.md](docs/toolchain-validation.md) lists the verified commands and known blockers, and [docs/compatibility.md](docs/compatibility.md) the tested versions.
4. Test Rust: `cargo test --workspace`
5. Generate bindings: `./scripts/generate-bindings.sh`
6. Build Counter for iOS: `cargo run -p forge-cli -- build ios --project examples/counter`

Contributing? [CONTRIBUTING.md](CONTRIBUTING.md) starts with a three-command path to a green local build.

Android (SDK + NDK on Linux) — see [docs/android-setup.md](docs/android-setup.md):

```bash
cargo run -p forge-cli -- build android --project examples/counter-android
```

CI assembles a **debug APK** on Ubuntu, runs Rust checks, binding drift tests, and validates the **Xcode project layout** for the Counter example. Full iOS device builds and Xcode compilation require macOS (see [docs/ios-xcode.md](docs/ios-xcode.md)); Linux iOS builds still use the Darwin SDK via xtool.

## Repository layout

```
crates/forge-core      # Domain logic (testable on Linux)
crates/forge-storage   # JSON snapshot persistence
crates/forge-net       # Networking trait (prototype: no-op client, no transport yet)
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
- No App Store distribution in this release. No real networking yet either: `forge-net` defines the intended `HttpClient` interface with a no-op client, but no transport is wired into `forge-ffi` or the apps.

- `rust-toolchain.toml` pins Rust; rustup picks it up automatically.
- `environment.json` bootstraps hosted coding-agent VMs (Ubuntu 24.04): its `install` command installs Swift, xtool, usbmuxd and the iOS Rust target, then runs `cargo fetch`. It is not machine-specific and is not needed for local development. See [docs/linux-setup.md](docs/linux-setup.md#hosted-agent-vms-environmentjson).
- `deny.toml` configures `cargo deny check` (advisories, licenses, sources).
