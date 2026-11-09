# Project generation (`forge init`)

## Contract

`forge init <Name> --bundle-id org.example.app [--platform ios|android] [--path <dir>] [--minimal]` creates an app at `<dir>`. Without `--path` it creates `./<Name>` in the current directory, or `examples/<Name>/` when you run it inside the Forge checkout. It then prints the next commands to run, and the app gets a `README.md` describing its layout.

The app **owns its Rust logic**: `rust/` is its own crate, with its own `Cargo.toml`, lock file and target directory, and `forge.toml` names it (`[rust] path = "rust"`). It depends on Forge's runtime crates (`forge-runtime`, `forge-storage`, `forge-net`) by path, and the Forge checkout supplies the build scripts and `uniffi-bindgen`. So an app is not fully standalone yet: it needs a Forge checkout on the same machine, but nothing of the app lives inside that checkout. See [app-crate.md](app-crate.md) for how to work in the crate.

### Quick start (Linux)

```bash
cd ~/projects
forge init MyApp --bundle-id org.example.myapp
cd MyApp
forge test       # the starter's Rust tests
forge run ios    # xtool: build and install on a connected iPhone
```

For Android, add `--platform android` and use `forge build android --install` (needs the Android SDK and NDK; see [android-setup.md](android-setup.md)). Inside an app, every command uses the current directory, so `--project` is only needed to point at another app.

### Finding the Forge checkout

`forge` must find the checkout: run it from inside the checkout, set `FORGE_WORKSPACE`, or use a `forge` installed with `cargo install --path crates/forge-cli` (which remembers its checkout). Relative `--project` paths are resolved against the current directory first, then the checkout, so `--project .` works from inside the app. `forge build`, `forge run`, `forge test` and `forge bindings generate` without `--project` use the current directory when it has a `forge.toml`.

### Paths written into the app

- A relative `--path` is resolved against the current directory. Missing parent directories are created.
- The framework an iOS app builds on (the Forge crates in `rust/Cargo.toml`, ForgeKit in `Package.swift`) comes from one of two places:
  - **a release**, for an app outside the checkout: the Forge crates from git at the commit in `forge-release.toml`, ForgeKit at exactly that version (`.package(url: "https://github.com/GeneralKnowledge/ForgeKit.git", exact: …)`). The app then builds whatever happens to the checkout;
  - **the checkout**, for an app inside it (like the examples), and for any app while the checkout's `crates/forge-app` version differs from the release in `forge-release.toml` (framework changes no release has yet): relative paths inside the checkout, the checkout's absolute path outside it. `forge init` says which it used.
- Android apps always use the checkout's crates (Android is work in progress).
- The iOS Xcode project's "Build Rust FFI" phase always points at the checkout's scripts, relatively or absolutely as above. If you move the checkout, update that path, and the crate and ForgeKit paths of an app that uses the checkout.
- Native libraries are the app's own and stay inside it: `rust/target/` (Cargo), `build/ios-device/lib` and `build/macos-host/lib` (iOS), `app/src/main/jniLibs` (Android). The generated `.gitignore` leaves them out of version control.

### iOS (`--platform ios`, default)

- `forge.toml` (app name, bundle ID, version, permissions, entitlements, signing: [app-config.md](app-config.md)), `Package.swift`, `.gitignore`, `README.md`
- `Info.plist`, `App.entitlements`, `xtool.yml`: generated from `forge.toml`, rewritten by every `forge build ios` / `forge run ios` (`forge config sync`)
- `rust/`: the app's Rust crate, a notes starter on the application runtime (`forge-app`; see [app-crate.md](app-crate.md))
- `Sources/AppHost/`: the SwiftUI app. `AppHost.swift` makes ForgeKit's `ForgeHost`, which opens the Rust core once per process, reports the lifecycle and tells the screens when Rust committed a change; `ContentView` shows it with `ForgeRoot`. The screens (notes list, editor, compose, settings) use the `@Observable` `NotesModel`, which calls the Rust API in `ForgeFFI` directly. See [ios-ui.md](ios-ui.md) for the conventions it follows
- `Sources/AppHostXtoolApp/`: xtool entry point; `Sources/AppHostLauncher/`: entry point for `swift run` and Xcode
- `Tests/AppHostTests/`: XCTest suite against the real Rust library (macOS: `forge test`)
- `Xcode/<Name>.xcodeproj`: conventional iOS application host (macOS)
- `Generated/`: Swift UniFFI output

### The minimal starter (`--minimal`, iOS)

`--minimal` starts from the same iOS template and Rust crate, removes the notes screens and native services, and lays `templates/ios-minimal/` over them: a one-screen checklist saved on the phone, on the same runtime and ForgeKit host, with `ItemsModel.swift`, `ContentView.swift`, its tests, a smoke UI test and its own README. [first-app.md](first-app.md) adds a feature to it. `MINIMAL_REMOVES` in `crates/forge-cli/src/commands.rs` lists what it removes, and a test checks that each of those files exists in the full template.

### Android (`--platform android`)

- `forge.toml` with `[android]` (min SDK, ABIs), `.gitignore`
- `rust/`: the app's Rust crate, from `templates/android-rust` (the shape from before the application runtime)
- Gradle + Compose host under `app/`: `NotesScreen`, `NotesViewModel`, and JVM unit tests against the real Rust library
- `Generated/`: Kotlin UniFFI output, plus `ForgeAndroid.kt` (HTTPS setup)

### Checked in CI

`scripts/check-fresh-app.sh` (CI job "Fresh app outside the checkout", or `make check-fresh-app`) runs `forge init --path` outside the checkout for both platforms (and `--minimal` for iOS), tests, lints and formats each app's crate on its own, checks that nothing was written into the checkout, then adds a Rust export and checks that `forge bindings generate --project` puts it in the Swift and Kotlin bindings. It also checks that the iOS app's Info.plist, App.entitlements and xtool.yml come from `forge.toml`, and that a permission, version, entitlement and `minimum_ios` edited in `forge.toml` reach them (`forge config sync`, `forge config check`). CI also runs, on apps created outside the checkout, the XCTest suite via `forge test` (macOS), an unsigned `xcodebuild` (macOS), and the JVM unit tests plus a debug APK via `forge build android` (Linux).

What CI cannot check is a physical device: see [toolchain-validation.md](toolchain-validation.md) for the manual device checklist and what has been recorded.

## Placeholders

Template sources use explicit tokens (never substring-replace product names):

| Token | Replaced with |
|-------|----------------|
| `FORGE_PACKAGE_NAME` | Project name (`<Name>`) |
| `FORGE_APP_NAME` | Same as project name in `forge.toml` |
| `FORGE_BUNDLE_ID` | `--bundle-id` argument |
| `FORGE_APP_SUPPORT_DIR` | Project name (Application Support subfolder, iOS) |
| `FORGE_JAVA_PACKAGE` | Same as `--bundle-id` (Android Kotlin/Gradle namespace) |
| `FORGE_WORKSPACE` | Path to the Forge checkout, in `rust/Cargo.toml` and the iOS `Package.swift` (relative or absolute), or the release pins instead (see above) |

## Requirements

- The destination (`examples/<Name>` or `--path`) must not already exist.
- Templates must contain the required files (see `validate_ios_template` / `validate_android_template` in `forge-cli`), and the starter crate must exist (`templates/app-rust` for iOS, `templates/android-rust` for Android).
- `cargo` must be able to build the starter crate on the host, because `forge init` generates the bindings from it.

## Regenerating bindings

After changing the exported Rust API:

```bash
forge bindings generate   # from the app directory, or --project <dir>
```

`forge build` and `forge run` regenerate them too, as does `forge test` when it runs host tests (Android, or iOS on macOS). Never edit `Generated/` by hand.

## Apps from before app-owned crates

Forge used to have a shared `forge-ffi` crate that apps named with `core_crate = "forge-core"` and `ffi_crate = "forge-ffi"`. It has been removed, and every bundled example now owns its crate like a `forge init` app. An old `forge.toml` is rejected with a message saying what to write instead: give the app a `rust/` crate (copy `templates/app-rust`) and set `[rust] path = "rust"`.
