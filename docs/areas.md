# Project areas

ixOS holds four things that are related but are not one product. Each part of the repository belongs to one of them:

| Area | What it is | Who it is for |
|---|---|---|
| **Framework** (Forge) | What apps run on: the Rust runtime and foundation crates, ForgeKit, ForgeUI, ForgeAppKit, native services | Every app, inside and outside this repository |
| **Tools** (Forge Tools) | What developers use to build, test and inspect an app: the `forge` CLI, device and UI-test crates, templates, Device Lab | A person (or agent) developing an app |
| **Infrastructure** | What runs this repository: CI, releases, build machines, toolchain pins, hosting | The maintainers |
| **Apps** | Products built with Forge: Pocket Library, Good Dog, Commonplace, FieldKit, Weather, Relay and the others | People who use the apps |

These are ownership boundaries, not separate repositories. Code moves only when a boundary has proved useful, and Forge Tools becomes its own repository only once it has a release cycle of its own.

## Dependencies run one way

Apps and tools both depend on the framework. The framework depends on neither.

| Area | May depend on |
|---|---|
| Framework | framework only |
| Apps | framework and other apps |
| Tools | framework and tools |

ForgeUI depends on nothing, and ForgeAppKit depends only on ForgeUI, so an app can use the design system without any Rust ([package strategy](forgeui.md)).

`scripts/check-boundaries.sh` (part of `make validate` and the Rust CI job) enforces this. Each Rust crate names its area in its `Cargo.toml`:

```toml
[package.metadata.forge]
area = "framework"   # or "tools", "app"
```

Crates under `crates/` must declare it. Crates under `examples/` are apps unless they say otherwise. The root Cargo workspace is for framework and tools crates, so an app crate in it counts as a crossing too.

Known crossings are listed in [boundaries.toml](boundaries.toml), each with a reason. Its status is either `fix`, with the plan that removes it, or `keep`, an allowed exception. The check fails on an unlisted crossing and on a listed one that no longer exists, so the list only shrinks. Today it holds:

- **Device Lab hosts every app** (`keep`). It is the test bed on the phone, so its crate links each app's crate.
- **Good Dog's core crate sits in the root workspace** (`fix`). It moves into Good Dog's own folder and workspace. (Chat's crates moved into Relay's `messaging/` workspace when Chat merged into Relay.)

The `forge` CLI also has app paths built in: some commands default `--project` to `examples/pocket-library`, and `forge check` runs named apps. A path is not a dependency, so the check doesn't see it. It is removed in the same round of fixes.

## What belongs where

### Framework

| Path | What |
|---|---|
| `crates/forge-runtime`, `forge-app`, `forge-storage`, `forge-net`, `forge-services` (with `swift/`), `forge-native`, `forge-reader` (with `swift/`) | Rust runtime and foundations |
| `forgekit/` | ForgeApp, ForgeKit, ForgeNative, ForgeUI, ForgeAppKit; released as GeneralKnowledge/ForgeKit |
| `examples/forgeui-showcase` | ForgeUI's own gallery |

### Tools

| Path | What |
|---|---|
| `crates/forge-cli`, `forge-device`, `forge-uitest`, `forge-crash`, `uniffi-bindgen` | The `forge` command and what it uses |
| `crates/forge-devproto` | Live dev connection protocol (used by forge-lab) |
| `templates/` | What `forge init` and `forge agent setup` copy |
| `examples/device-lab` | The test bed on the phone |
| `scripts/` build pipeline: `build-rust-*`, `forge-crate`, `generate-bindings`, `package-*-library`, `ios-no-dylib-linker`, `native-lib-needs-rebuild`, `write-native-build-meta` | Run by `forge build` |
| `scripts/` checks: `check-*`, `test-native-rebuild`, `test-swift-host`, `validate-xcode-project`, `verify-android-packaging`, `iphone-acceptance`, `ios-screenshots`, `ios-simulator`, `make-*-check-app`, `ai-check/`, `a11y-report` (report only) | Repository and device checks |

forge-lab, the agent tooling, is a separate repository built on these tools.

### Infrastructure

| Path | What |
|---|---|
| `.github/workflows/` | CI |
| `scripts/forgekit.sh`, `test-forgekit-release.sh`, `forge-release.toml` | Framework releases |
| `scripts/setup-build-host.sh`, `install-xtool.sh`, `validate-toolchain.sh`, `validate-android-toolchain.sh`, `ci-macos.sh` | Build machines |
| `toolchain/`, `rust-toolchain.toml`, `environment.json`, `deny.toml` | Toolchain pins and policy |
| `examples/relay/messaging/chat-server/deploy` | Chat's server hosting |

The build queue, signing and `forge publish ios` are infrastructure jobs that live inside the CLI.

### Apps

Everything else under `examples/`: Pocket Library (and its Android version), Good Dog (with `good-dog-core`), Commonplace, FieldKit, Weather, Checklist, Relay (with `verification/relay`, and its messaging: `chat-core`, `chat-server`, `chat-cli`), Sketchbook and Soundscape. `scripts/pocket-library-screenshots.sh` and `weather-screenshots.sh` belong to their apps.

## Each area's responsibilities

| Area | Version | Release | CI | Roadmap | Start reading |
|---|---|---|---|---|---|
| Framework | forge-app version; ForgeKit semver in `forge-release.toml` | `scripts/forgekit.sh release` | `rust.yml`, `forgeui.yml`, `app-model.yml` | P0 to P3 | [architecture.md](architecture.md) |
| Tools | `forge` CLI version | Built from source | `rust.yml`, `check-human-workflow.sh` | P5; [tooling freeze](roadmap.md#tooling-freeze-and-reuse-rules) | [getting-started.md](getting-started.md) |
| Infrastructure | Toolchain pins | None of its own | `macos.yml` (manual), the build queue | Builds, signing, App Store | [builds.md](builds.md) |
| Apps | Each app's `forge.toml` | Device Lab by cable; App Store later | App crates, [app parity](app-parity.md), `screenshots.yml`, `android.yml` | Apps-first waves | Each app's README |

## Plan

Agreed 2026-11-16: split ownership first, then move folders.

1. **Boundaries** (this page, the area tags and the check).
2. **Fix the crossings** listed above, each in its own small PR.
3. **Rename `examples/` to `apps/`**, with Device Lab moving to `tools/`, once no app PR is open. `crates/` stays flat, tagged by area, until Forge Tools has its own release cycle.
