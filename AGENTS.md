# Instructions for coding agents

ixOS (Forge Mobile) builds genuinely native iOS apps from Linux: SwiftUI for the interface, Rust for the logic, a UniFFI boundary between them, and xtool to build, sign and install on a physical iPhone. Read this file before changing anything; it is the short version of the rules spread across `docs/`.

## What Forge is and isn't

- It makes Apple's frameworks productive from Linux. It does not replace SwiftUI, and it is not a cross-platform UI layer or a game engine.
- iOS is the focus. Android is work in progress: keep it building, add no Android features ([roadmap](docs/roadmap.md#android--work-in-progress)).
- Linux first. Write instructions for Linux with xtool; macOS and Xcode are optional extras, never assumed.
- Human first, agent optional. Every step of developing an app works from the `forge` CLI for a person with no AI agent, MCP server or API key ([human-workflow.md](docs/human-workflow.md)). Agents get the same commands, plus optional integration (`forge mcp`, `forge agent setup`).

## Layout

| Path | What |
|---|---|
| `crates/forge-runtime` | Background work, cancellation, event streams, lifecycle, `app_shell!`, logging |
| `crates/forge-services` | Native services: Rust traits in `src/`, Swift adapters in `swift/` (copied into each app's `Generated/`) |
| `crates/forge-storage`, `crates/forge-net` | Persistence (JSON, SQLite) and networking |
| `forgekit/` | The Swift package: ForgeKit (the runtime host) and ForgeUI (the design system), released as GeneralKnowledge/ForgeKit |
| `crates/forge-cli` | The `forge` command |
| `crates/forge-device`, `forge-devproto`, `forge-devtools` | Talking to a USB iPhone from Linux, and the live session with development builds |
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
9. **A capability lives in the CLI, not only in MCP.** Build it as a `forge` command (with `--json`) or in a library the CLI uses; `forge mcp` runs that command and adds only schemas, argument checks and session state. A default app needs no agent files, and safety rules (exploration, installing, `forge verify --accept`) are the same whoever calls. `scripts/check-human-workflow.sh` must keep passing.
10. **xtool changes** go to the fork (`GeneralKnowledge/xtool`). Do not open PRs, issues or comments on upstream xtool.
11. **Agent and device tooling is frozen** (`forge ui explore`, `forge ui focus`, run records, `forge mcp`, the live dev connection): bug fixes only, no new commands, until the roadmap's P1 to P4 exit criteria are met or the freeze is reviewed on 2026-12-15 ([roadmap](docs/roadmap.md#tooling-freeze-and-reuse-rules)). The one exception is interaction completeness: basic gestures, keys and device navigation in `forge-uitest`, `forge ui` and `forge_ui_act`'s action list. Before building something, check for an existing tool or Apple framework that does it.

## Checking your work

Rust and everything that runs on Linux:

```bash
cargo test --workspace
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
make validate        # the above plus binding drift and template sync
```

After changing an app crate's exported API: `forge bindings generate --project examples/<app>`, then `./scripts/check-app-crates.sh` and `./scripts/check-bindings.sh`.

Swift: SwiftUI compiles only with the Darwin SDK (xtool on a Linux laptop that has it, `forge build ios --project <app>`) or on macOS CI (`.github/workflows/macos.yml`, about 35 minutes including the queue). In a session without the SDK, say that Swift changes were checked by CI only.

## Device tools

On a machine with an iPhone over USB: `forge device list|info|logs|crashes` (`crashes --pull <dir>` symbolicates; `forge crash <file.ips>` reads a report offline), and for a development build (`forge run ios --devtools`) `forge device pair|status|snapshot|cmd|connect`. See [forge-device.md](docs/forge-device.md). The agent-facing design of the dev loop and the MCP server is in [dev-loop.md](docs/dev-loop.md) and [forge-mcp.md](docs/forge-mcp.md).

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
