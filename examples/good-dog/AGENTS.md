# Instructions for coding agents: GoodDog

GoodDog is a native iOS app made with Forge: the logic and state are Rust (`rust/`), the screens are SwiftUI (`Sources/AppHost/`), and it is built, signed and installed on an iPhone from Linux with xtool. `README.md` explains the layout for people; this file is the rules.

## Rules

1. **Rust owns the state and the rules.** Validation, persistence, networking and anything worth testing go in `rust/src/`. Swift shows Rust's state and sends it commands; it does not re-implement rules.
2. **The model stays free of UI frameworks.** `Sources/AppHost/GoodDogModel.swift` and `Display.swift` import `ForgeFFI`, Foundation and Observation only. Keep it (and any new model file) that way, so it compiles and tests without the iOS SDK. SwiftUI and UIKit belong in view files.
3. **Accessibility identifiers on every interactive control,** named `screen.element` (for example `today.shorten`, `record.save`), plus a label a VoiceOver user understands. UI tests find controls by identifier, never by screen coordinates.
4. **Native first.** Use SwiftUI's own navigation, lists, forms, sheets and SF Symbols. Support light and dark mode and Dynamic Type up to the largest accessibility size.
5. **Never edit `Generated/`.** After changing what `rust/src/lib.rs` exports, run `forge bindings generate`.
6. **Never edit `Info.plist`, `App.entitlements` or `xtool.yml`.** They are generated from `forge.toml` (`forge config show` prints the effective settings).
7. **Log with `forge_runtime::logging`** in Rust. Never log user content.

## The loop

With an MCP client, use the `forge` server in `.mcp.json` (`forge mcp`): its tools check, build, install, launch, inspect, test and diagnose, and every result says `ok`, the failing `stage`, compiler `diagnostics` with file and line, and `hints`. Read its `forge://conventions` resource first. On the command line, the same steps are:

```bash
forge check swift                            # the model and bindings with plain Swift on Linux, seconds; not the SwiftUI views
cargo test --manifest-path rust/Cargo.toml   # Rust logic, seconds, no SDK or phone
forge bindings generate                      # after changing the Rust API
forge build ios                              # Rust + Swift for iOS (needs xtool and the Darwin SDK)
forge run ios                                # build, sign and install on the connected iPhone
forge device logs --process GoodDogXtool --for 20s   # the app's log on the phone
forge device crashes --pull ./crashes        # crash reports from the phone, symbolicated
forge crash ./crashes/<report>.ips           # read one again: kind, likely cause, file and line
```

Add `--json` to any of these for one `forge.result/1` object on stdout instead of prose.

`forge run ios` does not open the app; with the UI testing runner set up (`forge ui setup`), the MCP tool `forge_launch_app` does, otherwise someone taps its icon. Installing is the person's call: `forge mcp` refuses it unless they started it with `--allow install`. `forge doctor` says which Apple team signs: a free team allows 3 sideloaded apps per phone and re-signing every 7 days, a paid team neither. Check with the person before installing a new app or removing one.

`forge test` runs the Rust tests, and the XCTest suite only on macOS. Without the iOS SDK, SwiftUI is checked only by a Mac or by `forge build ios` on a machine that has the SDK; say so when you report.

## Where to look

- Forge's docs are in the Forge checkout (its path is in `rust/Cargo.toml`, in the `forge-runtime` dependency): `docs/ios-ui.md` (SwiftUI standard), `docs/app-crate.md` (the Rust crate), `docs/native-services.md` (Keychain, notifications, permissions, files, sharing, deep links), `docs/app-config.md` (`forge.toml`), `docs/capability-ledger.md` (what is proven on an iPhone), `docs/troubleshooting.md`.
- Apple's documentation: the `sosumi` server in `.mcp.json` serves developer.apple.com pages and the Human Interface Guidelines as text.

## Project notes

Record decisions and known issues about this app here, newest first, so the next session doesn't rediscover them.

- 2026-11-08 (GD1): the plan engine, dog rules and content are `examples/good-dog-core` (ported from the Good Dog web app; parity tests there). This crate adds saving (`gooddog.json`, one JSON document), the day boundary (`LocalTime`: the host passes the time and its UTC offset) and the API. Change planning rules in good-dog-core, not here.
- Goals are a closed set (`TrainingGoal`), mapped to the phrases the engine's keyword matching expects. The web app used free text.
- `-ForgeUITestData` starts on a fresh throwaway folder every launch, so a device test can't check persistence across a relaunch; the twin tests do (GD-2).
