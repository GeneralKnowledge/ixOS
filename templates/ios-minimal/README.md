# FORGE_APP_NAME

An iOS app made with [Forge](https://github.com/GeneralKnowledge/ixOS) from the minimal starter: a checklist saved on the phone. The logic is Rust, the screen is SwiftUI, and you build and install it on an iPhone from Linux.

## Run it

```bash
forge run ios    # build the Rust and Swift, sign and install on the iPhone, then open it there
forge test       # the Rust tests (and the XCTest suite on macOS)
```

The first time, `forge run ios` explains the one-time steps on the phone (Trust, Developer Mode, trusting your developer profile). If the machine isn't set up yet, run `forge setup ios` first.

## What goes where

| I want to… | I edit |
|---|---|
| Change a rule (what counts as a valid item, what toggling does) | `rust/src/lib.rs`, in `AppCore` |
| Store more data (a new field, a new list) | `rust/src/lib.rs`: the `Item` record and `Saved` |
| Change how something looks | `Sources/AppHost/ContentView.swift` |
| Make a button do something new | Add a method to `AppCore` (Rust), call it from `ItemsModel.swift`, add the button in `ContentView.swift` |
| Add a screen | A new `View` file in `Sources/AppHost/`, opened from `ContentView` with a `NavigationLink` or `.sheet` |

Rust owns the state and the rules; Swift shows them. Each change is a Rust command that saves before it returns, on the Forge application runtime; ForgeKit then tells the screen to re-read. `ItemsModel.swift` is the bridge: it calls Rust and keeps what the screen shows. It imports no SwiftUI, so `forge check swift` compiles and tests it on Linux in seconds.

[docs/first-app.md](https://github.com/GeneralKnowledge/ixOS/blob/main/docs/first-app.md) in the Forge checkout walks through adding a feature to this app.

## Files

| Path | What it is |
|------|------------|
| `rust/src/lib.rs` | The app's logic and saved data. Everything marked `#[uniffi::export]` is callable from Swift. |
| `Sources/AppHost/AppHost.swift` | Opens the Rust core once per process (ForgeKit's `ForgeHost`) and follows the app lifecycle. |
| `Sources/AppHost/ItemsModel.swift` | The view model: calls Rust, holds what the screen shows. |
| `Sources/AppHost/ContentView.swift` | The screen. |
| `Tests/AppHostTests/` | Tests of the view model against the real Rust core. |
| `uitests/smoke.toml` | A UI test that drives the app on a plugged-in iPhone (`forge ui run uitests/smoke.toml`). |
| `Generated/` | Swift bindings generated from `rust/`. Do not edit; `forge run ios` rewrites them. |
| `forge.toml` | App name, bundle identifier, version, permissions (`forge config show`). |
| `Info.plist`, `App.entitlements`, `xtool.yml` | Generated from `forge.toml` on every build. Edit `forge.toml` instead. |
| `Xcode/` | An Xcode project, for a Mac. Never required. |
| `AGENTS.md`, `CLAUDE.md`, `.mcp.json` | Instructions and tools for AI coding agents. |

## When you need more

The full starter (`forge init` without `--minimal`) shows events from Rust, cancellable networking, the app lifecycle, native services (Keychain, notifications, deep links) and logging from Swift. Read it when you need one of those; see `docs/app-crate.md` in the Forge checkout.

## Help

- Something missing: `forge doctor`.
- A build or install failed: the error ends with "What went wrong" and a fix.
- It installs but the screen is blank or it closes: `forge device logs` and `forge device crashes` (`docs/troubleshooting.md`).
