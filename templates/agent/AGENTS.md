# Instructions for coding agents: FORGE_APP_NAME

FORGE_APP_NAME is a native iOS app made with Forge: the logic and state are Rust (`rust/`), the screens are SwiftUI (`Sources/AppHost/`), and it is built, signed and installed on an iPhone from Linux with xtool. `README.md` explains the layout; this file lists the rules that are easy to get wrong. Everything below works from the command line, the same way a person works on the app. The MCP server in `.mcp.json` is an optional convenience over the same commands.

## Rules

1. **Rust owns the state and the rules.** Validation, persistence, networking and anything worth testing go in `rust/src/`. Swift shows Rust's state and sends it commands; it does not re-implement rules.
2. **Model files stay free of UI frameworks.** The `*Model.swift` files in `Sources/AppHost/` import the app's bindings (`[swift] bindings_module` in `forge.toml`), `ForgeKit`, Foundation and Observation only, so they compile and test without the iOS SDK (`forge check swift`). SwiftUI and UIKit belong in view files. `forge inspect` flags a model file that breaks this.
3. **Accessibility identifiers on every interactive control,** named `screen.element` (for example `notes.add`), plus a label a VoiceOver user understands. UI tests find controls by identifier, never by screen coordinates.
4. **Native first.** SwiftUI's own navigation, lists, forms, sheets and SF Symbols. Light and dark mode, and Dynamic Type up to the largest accessibility size.
5. **Never edit `Generated/`, `Info.plist`, `App.entitlements` or `xtool.yml`.** Bindings come from `forge bindings generate`; the rest from `forge.toml` (`forge config show`).
6. **Log with `forge_runtime::logging`** in Rust. Never log user content.
7. **Installing, removing apps and accepting results are the person's.** Build without asking: `forge run ios` compiles before it looks for the phone and stops with the build ready if there is none, so start it as soon as there is something to try and ask for the phone only then. Ask before `forge run ios` installs an app that isn't on the phone yet (a free Apple team allows 3 sideloaded apps and 10 new App IDs a week; `forge doctor` says which team signs). Never run `forge verify --accept`: only a person records an acceptance.
8. **Say what you didn't check.** Without the iOS SDK, SwiftUI is not compiled; a Linux check is not an iPhone result.

## Commands

```bash
forge inspect                                # the app's files: Swift imports, identifiers, tests, generated files
cargo test --manifest-path rust/Cargo.toml   # Rust logic, seconds, no SDK or phone
forge check swift                            # model and bindings with plain Swift on Linux; not the SwiftUI views
forge bindings generate                      # after changing what rust/src/lib.rs exports
forge build ios                              # Rust + Swift for iOS (needs xtool and the Darwin SDK)
forge run ios                                # build, then sign and install on the connected iPhone (no phone: stops once built)
forge ui launch                              # open it (after `forge ui setup`); --appearance dark, --largest-text
forge ui run uitests/smoke.toml              # a UI test; evidence in .forge/runs/
forge device logs --process FORGE_XTOOL_PRODUCT --for 20s   # the app's log on the phone
forge diagnose                               # why isn't it working: phone, install, crash, log
forge verify                                 # requirements and their current evidence
```

Add `--json` to any command for one `forge.result/1` object on stdout: `ok`, the failing `stage`, compiler `diagnostics` with file and line, and `hints`.

## Where to look

Forge's docs are in the Forge checkout (its path is in `rust/Cargo.toml` or on GitHub): `docs/ios-ui.md` (SwiftUI standard), `docs/app-crate.md` (the Rust crate), `docs/native-services.md`, `docs/app-config.md` (`forge.toml`), `docs/forge-ui-testing.md`, `docs/troubleshooting.md`.

## Project notes

Record decisions and known issues about this app here, newest first, so the next session doesn't rediscover them.

- (none yet)
