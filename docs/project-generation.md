# Project generation (`forge init`)

## Contract (v0.1)

`forge init <Name> --bundle-id org.example.app` creates **`examples/<Name>/`** inside the Forge Mobile **workspace**. Generated apps are **not standalone** repositories in this version.

Each project includes:

- `forge.toml` with validated app/rust/build sections
- `Package.swift` with workspace-relative Rust library path (`../../build/ios-device/lib`)
- `xtool.yml` with the supplied bundle identifier
- `Sources/AppHost/` SwiftUI host (Counter sample UI)
- `Generated/` UniFFI outputs produced by `./scripts/generate-bindings.sh` at init time (not copied from template)

## Placeholders

Template sources use explicit tokens (never substring-replace product names):

| Token | Replaced with |
|-------|----------------|
| `FORGE_PACKAGE_NAME` | Project name (`<Name>`) |
| `FORGE_APP_NAME` | Same as project name in `forge.toml` |
| `FORGE_BUNDLE_ID` | `--bundle-id` argument |
| `FORGE_APP_SUPPORT_DIR` | Project name (Application Support subfolder) |

## Requirements

- Destination `examples/<Name>` must not already exist.
- Template must contain required files (see `validate_template` in `forge-cli`).
- Host workspace must build `forge-ffi` so binding generation succeeds.

## Build workflow for generated apps

From repository root:

```bash
./scripts/package-ios-library.sh
cd examples/<Name> && xtool dev
```

Rust crates (`forge-core`, `forge-ffi`) are always built from the workspace root, not duplicated per app.
