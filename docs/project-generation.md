# Project generation (`forge init`)

## Contract (v0.1)

`forge init <Name> --bundle-id org.example.app [--platform ios|android]` creates **`examples/<Name>/`** inside the Forge Mobile **workspace**. Generated apps are **not standalone** repositories in this version.

### iOS (`--platform ios`, default)

- `forge.toml`, `Package.swift`, `xtool.yml`
- `Sources/AppHost/` SwiftUI host
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

- Destination `examples/<Name>` must not already exist.
- Template must contain required files (see `validate_template` in `forge-cli`).
- Host workspace must build `forge-ffi` so binding generation succeeds.

## Build workflow for generated apps

From repository root:

```bash
./scripts/package-ios-library.sh examples/<Name>/Generated
cd examples/<Name> && xtool dev
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
