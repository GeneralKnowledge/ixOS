# Project generation (`forge init`)

## Contract (v0.1)

`forge init <Name> --bundle-id org.example.app [--platform ios|android] [--path <dir>]` creates **`examples/<Name>/`** inside the Forge Mobile **workspace**, or `<dir>` when `--path` is given. Generated apps are **not standalone** in this version: they build against the workspace's Rust crates, scripts and native libraries.

### Creating an app outside the workspace (`--path`)

```bash
forge init MyApp --bundle-id org.example.myapp --path ~/projects/MyApp
forge build ios --project ~/projects/MyApp
```

- A relative `--path` is resolved against the current directory. Missing parent directories are created.
- The templates assume `examples/<Name>` (workspace two levels up). `forge init` rewrites those references for the actual location: an app **inside** the checkout keeps a relative path (so the checkout can move as a whole); an app **outside** gets the checkout's **absolute** path. If you later move the checkout, update the path in `Package.swift` and `Xcode/<Name>.xcodeproj/project.pbxproj` (iOS) or `app/build.gradle.kts` (Android).
- The rewritten references are the native library search paths in `Package.swift`, the "Build Rust FFI" phase in the Xcode project, and the JNA library path for Android JVM unit tests.
- `forge` must still find the workspace: run it from inside the checkout, set `FORGE_WORKSPACE`, or use a `forge` installed with `cargo install --path crates/forge-cli` (which remembers its checkout). `--project` paths that are relative are resolved against the workspace, so pass the app's absolute path (the one `forge init` prints).

### iOS (`--platform ios`, default)

- `forge.toml`, `Package.swift`, `xtool.yml`
- `Sources/AppHost/` SwiftUI library + `Sources/AppHostXtoolApp/` xtool entry point + `Sources/AppHostLauncher/` entry point for `swift run` and Xcode
- `Xcode/<Name>.xcodeproj` conventional iOS application host (macOS)
- `Generated/` Swift UniFFI output at init time

### Android (`--platform android`)

- `forge.toml` with `[android]` (min SDK, ABIs)
- Gradle + Compose host under `app/`
- `Generated/` Kotlin UniFFI output at init time
- JNI libraries built from repo root (`scripts/build-rust-android.sh`)

Both platforms require `core_crate` / `ffi_crate` = `forge-core` / `forge-ffi` in v0.1.

## Placeholders

Template sources use explicit tokens (never substring-replace product names):

| Token | Replaced with |
|-------|----------------|
| `FORGE_PACKAGE_NAME` | Project name (`<Name>`) |
| `FORGE_APP_NAME` | Same as project name in `forge.toml` |
| `FORGE_BUNDLE_ID` | `--bundle-id` argument |
| `FORGE_APP_SUPPORT_DIR` | Project name (Application Support subfolder, iOS) |
| `FORGE_JAVA_PACKAGE` | Same as `--bundle-id` (Android Kotlin/Gradle namespace) |

## Requirements

- The destination (`examples/<Name>` or `--path`) must not already exist.
- Template must contain required files (see `validate_template` in `forge-cli`).
- Host workspace must build `forge-ffi` so binding generation succeeds.

## Build workflow for generated apps

From repository root:

```bash
./scripts/package-ios-library.sh examples/<Name>/Generated
cd examples/<Name> && xtool dev run -c release
```

Or use the CLI (bindings path follows `--project`):

```bash
cargo run -p forge-cli -- build ios --project examples/<Name>
```

### Android generated apps

```bash
./scripts/package-android-library.sh examples/<Name>/Generated
cd examples/<Name> && ./gradlew :app:assembleDebug
```

```bash
cargo run -p forge-cli -- build android --project examples/<Name>
```

Rust crates (`forge-core`, `forge-ffi`) are always built from the workspace root, not duplicated per app.
