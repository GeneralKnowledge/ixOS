# Instructions for coding agents

ixOS (Forge Mobile) builds genuinely native iOS apps from Linux: SwiftUI for the interface, Rust for the logic, a UniFFI boundary between them, and xtool to build, sign and install on a physical iPhone. Read this file before changing anything; it is the short version of the rules spread across `docs/`.

## What Forge is and isn't

- It makes Apple's frameworks productive from Linux. It does not replace SwiftUI, and it is not a cross-platform UI layer or a game engine.
- iOS is the focus. Android is work in progress: keep it building, add no Android features ([roadmap](docs/roadmap.md#android--work-in-progress)).
- Linux first. Write instructions for Linux with xtool; macOS and Xcode are optional extras, never assumed.
- Human first, agent optional. Every step of developing an app works from the `forge` CLI for a person with no AI agent, MCP server or API key ([human-workflow.md](docs/human-workflow.md)). Agents get the same commands, plus `forge agent setup`. The MCP server, the explorer, run records and the live dev connection live in the separate [forge-lab](https://github.com/GeneralKnowledge/forge-lab) side project.

## Layout

| Path | What |
|---|---|
| `crates/forge-runtime` | Background work, cancellation, event streams, lifecycle, `app_shell!`, logging |
| `crates/forge-services` | Native services: Rust traits in `src/`, Swift adapters in `swift/` (copied into each app's `Generated/`) |
| `crates/forge-storage`, `crates/forge-net` | Persistence (JSON, SQLite) and networking |
| `forgekit/` | The Swift package: ForgeKit (the runtime host) and ForgeUI (the design system), released as GeneralKnowledge/ForgeKit |
| `crates/forge-cli` | The `forge` command |
| `crates/forge-device`, `forge-devproto`, `forge-uitest`, `forge-crash` | Talking to a USB iPhone from Linux (logs, crashes, UI tests); `forge-devproto` is the encrypted channel Relay uses |
| `examples/` | Apps. Each Forge app owns its Rust crate in `rust/`; `forgeui-showcase` is a plain SwiftUI app on ForgeUI only |
| `templates/` | What `forge init` copies; `templates/agent/` is what `forge agent setup` adds |
| `docs/` | Developer docs. `docs/research/` holds investigations |

## Rules

1. **Framework vs app.** Reusable infrastructure goes into the framework; an app's identity and domain logic stay in the app. Extract only on a real second use, or when the code is clearly platform-level, and only after the PR that introduced it has merged ([architecture.md](docs/architecture.md#framework-principles)).
2. **Rust owns state and rules; Swift presents them.** Follow [ffi-policy.md](docs/ffi-policy.md) for async, events, cancellation and errors across the boundary, and [ios-ui.md](docs/ios-ui.md) for SwiftUI.
3. **Keep view models free of UI frameworks.** Model files import Foundation and Observation (and `ForgeFFI`) only, so they compile and test on Linux. SwiftUI and UIKit belong in view files.
4. **Accessibility identifiers on every interactive control,** named `screen.element` (for example `library.add`). UI tests find elements by identifier, then label, never by coordinates. Give controls labels a VoiceOver user can understand.
5. **Log through `forge_runtime::logging`** in Rust. Never log user content.
6. **Never edit `Generated/` by hand.** Regenerate with `forge bindings generate --project <app>` and commit the result.
7. **Update the [capability ledger](docs/capability-ledger.md) in the same PR as a capability change.** Edit or add its record under `docs/ledger/` and run `forge ledger generate`, which also rewrites the summary in `docs/status.md`; never edit either generated page. "iPhone: Yes" needs a run on a physical iPhone reported by a person; never record a skipped run as passing.
8. **Physical device steps:** say which step needs the iPhone and what it proves. Don't claim device results you didn't see.
9. **A capability lives in the CLI.** Build it as a `forge` command (with `--json`, [json-output.md](docs/json-output.md)) or in a library the CLI uses; tools built on top (forge-lab's MCP server) run that command. A default app needs no agent files, and safety rules (installing, `forge verify --accept`) are the same whoever calls. `scripts/check-human-workflow.sh` must keep passing.
10. **xtool changes** go to the fork (`GeneralKnowledge/xtool`). Do not open PRs, issues or comments on upstream xtool.
11. **Agent tooling lives in [forge-lab](https://github.com/GeneralKnowledge/forge-lab), not here** (the MCP server, `explore`, `focus`, run records, the live dev connection). Don't add it back to ixOS. Device and UI-test tooling here (`forge ui`, `forge device`) gets bug fixes and interaction completeness only (basic gestures, keys and device navigation) ([roadmap](docs/roadmap.md#tooling-freeze-and-reuse-rules)). Before building something, check for an existing tool or Apple framework that does it.

## Checking your work

Rust and everything that runs on Linux:

```bash
cargo test --workspace
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
make validate        # the above plus binding drift and template sync
```

After changing an app crate's exported API: `forge bindings generate --project examples/<app>`, then `./scripts/check-app-crates.sh` and `./scripts/check-bindings.sh`.

Swift: SwiftUI compiles only with the Darwin SDK. The check is `scripts/check-ios-xtool.sh` on a Linux machine with xtool (the maintainer's laptop, which agents reach through Remote Control): `affected <base>` builds the apps a change touches for an iPhone, unsigned, with no phone needed, and `--ref <branch>` builds a pushed branch without touching the laptop's checkout ([ios-build-check.md](docs/ios-build-check.md)). The macOS workflow (`.github/workflows/macos.yml`, about 35 minutes) is a manual fallback: don't start it unless the laptop can't be reached or the person asks. In a session that ran neither, say that Swift changes were not compiled.

## Device tools

Publish every build meant for the phone with `forge publish ios --project <app>`, from the laptop. It builds and signs without the phone and adds the build to the one Install page on the tailnet, labelled with its branch and commit; earlier builds, from other branches too, stay on the page. Then tell the person the build is there: they install it from Safari whenever and wherever they like, so no step waits on the phone ([over-the-air.md](docs/over-the-air.md)). Use the cable only for what needs it: logs, crash pulls and `forge ui`. Every app is reachable from Device Lab's Apps screen, and Device Lab is the app on the phone that gets tested.

For testing on the phone, publish the combined Device Lab (`--project examples/device-lab`), which hosts every app (decided 2026-11-15). The Install page only updates an app that is already on the phone: with a free Apple team, iOS refuses an app's first install over the air, so each app goes on once by cable (`forge run ios`) and every later build comes from the page. One unified app keeps that to a single cable install. Publish an app on its own only when the person asks; it then needs its own first cable install, and a free team allows only 3 installed apps. It also allows only 10 new App IDs a week, so always reuse an app's existing bundle id ([builds.md](docs/builds.md#build-speed)).

The laptop is shared, and parallel builds have frozen it, so every heavy build goes through one queue: one at a time, in arrival order, each in a memory and CPU cap that kills a runaway build before it can freeze the machine ([builds.md](docs/builds.md#the-build-queue)). `forge build ios`, `forge run ios`, `forge publish ios` and `scripts/check-ios-xtool.sh` queue by themselves. Anything else heavy on the laptop (`cargo build`, `cargo test`, `swift build`, `xtool dev build`, a build script) runs only as `forge queue run --note "<thread or PR>" -- <command>`, never directly. Use the forge in `~/.cargo/bin` or one built from current main (older copies don't queue), set `FORGE_BUILD_LOCK_NOTE` to your thread or PR, check `forge queue status` before saying a build has started, and never start a second build to get around the wait. The queue itself is finished: bug fixes only, no new features ([roadmap](docs/roadmap.md#tooling-freeze-and-reuse-rules)).

Never make a build wait on the phone. `forge run ios` builds first and looks for the iPhone only at the end; with no phone it stops with the build ready, and the next run rebuilds only what changed and installs. So when a change needs a phone run, start `forge run ios` on the laptop in the same turn, fix any compile errors, and ask the person for the phone only once it says the build is ready: an approval for installing then covers just the quick install. To reuse a build check's work, install from the checkout it built: `scripts/check-ios-xtool.sh` ends by printing the `forge run ios --project …` line for each app it built (with `--ref`, that is the reused worktree), and both build debug Swift on forge-dev Rust, so the install recompiles nothing.

On a machine with an iPhone over USB: `forge device list|info|logs|crashes` (`crashes --pull <dir>` symbolicates; `forge crash <file.ips>` reads a report offline). See [forge-device.md](docs/forge-device.md).

## Docs to read for a task

Every doc is listed by task in [docs/README.md](docs/README.md).

| Task | Read |
|---|---|
| A new app or app feature | [getting-started.md](docs/getting-started.md), [app-crate.md](docs/app-crate.md), [ios-ui.md](docs/ios-ui.md), [forgeui.md](docs/forgeui.md) |
| A native iOS capability | [native-services.md](docs/native-services.md), the ledger, [framework-extraction-audit.md](docs/framework-extraction-audit.md) |
| Saving app data (files, documents, databases, schema changes) | [persistence.md](docs/persistence.md) |
| App settings, permissions, entitlements | [app-config.md](docs/app-config.md) |
| Proving a feature works (requirements, evidence) | [evidence.md](docs/evidence.md) |
| Noticing when a UI change made a screen worse (screenshot baselines) | [visual-regression.md](docs/visual-regression.md) |
| Changing a foundation crate (runtime, storage, net, services) or the FFI rules | [foundation-review.md](docs/foundation-review.md): open findings and the `gap_*` tests that pin them |
| A build or install failure | [troubleshooting.md](docs/troubleshooting.md), [linux-setup.md](docs/linux-setup.md) |
