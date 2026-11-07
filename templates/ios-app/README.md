# FORGE_APP_NAME

An iOS app made with [Forge](https://github.com/GeneralKnowledge/ixOS): the logic is Rust, the screen is SwiftUI, and you can build and install it on an iPhone from Linux with xtool.

## Run it

From this directory:

```bash
forge doctor     # is the toolchain ready? (xtool, Darwin SDK, Apple sign-in, iPhone)
forge run ios    # build the Rust and Swift, sign, install and launch on the iPhone
forge test       # Rust tests (and the XCTest suite on macOS)
```

`forge run ios` checks the toolchain, your Apple sign-in and the connected iPhone before it starts building, and when a step fails it says which part failed (Rust, Swift, signing or the iPhone) and how to fix it.

The first time you run on an iPhone:

1. Connect it by USB, unlock it and tap **Trust**. `forge devices` should list it.
2. Turn on **Developer Mode** (Settings > Privacy & Security > Developer Mode), then restart the phone.
3. Sign xtool in to your Apple account: `xtool auth login`. Any Apple ID works; a free one can have 3 apps installed at a time and needs a re-install every 7 days.
4. After the first install, trust your developer profile: Settings > General > VPN & Device Management > your Apple ID > Trust.

## Where things are

| Path | What it is |
|------|------------|
| `rust/src/lib.rs` | Your app's logic, state, persistence and networking. Everything marked `#[uniffi::export]` is callable from Swift. |
| `rust/Cargo.toml` | Rust dependencies. Add crates here as usual (`cargo add --manifest-path rust/Cargo.toml <crate>`). |
| `Sources/AppHost/` | The SwiftUI screen (`ContentView.swift`) and the model that calls Rust. |
| `Tests/AppHostTests/` | XCTest suite against the real Rust library (macOS only). |
| `Generated/` | Swift bindings generated from `rust/`. Do not edit; `forge run ios` and `forge bindings generate` rewrite them. |
| `forge.toml` | App name, bundle identifier, version, minimum iOS, permissions, entitlements (`forge config show`). |
| `Info.plist`, `App.entitlements`, `xtool.yml` | Generated from `forge.toml` by every `forge build ios` / `forge run ios`. Edit `forge.toml` instead. |
| `Xcode/` | An Xcode project, for when you are on a Mac and want the debugger. Never required. |
| `build/`, `rust/target/` | Build output. Safe to delete. |

## The loop

1. Change the Rust in `rust/src/lib.rs`. `cargo test --manifest-path rust/Cargo.toml` tests it in seconds, with no SDK or phone.
2. If you changed what is exported, `forge bindings generate` updates the Swift API so your editor sees it.
3. Use it from `Sources/AppHost/`, then `forge run ios`.

## Help

- Something missing: `forge doctor` lists each missing piece and the command that fixes it.
- A build or install failed: the error ends with "What went wrong" and a fix. The Forge checkout's `docs/troubleshooting.md` has more.
- Getting started from scratch: `docs/getting-started.md` in the Forge checkout.
