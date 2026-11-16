# Forge docs

Forge builds native iPhone apps from Linux. You write the screens in SwiftUI and the logic in Rust, and Forge builds the app, signs it and installs it on your iPhone. ixOS is the name of the repository; Forge is the tool it gives you. No Mac is needed.

**New here?** Start with [getting-started.md](getting-started.md).

**Does it work yet?** [status.md](status.md) lists what works on an iPhone today, what is built but not yet proven on a phone, and what isn't built. It is generated from the [capability ledger](capability-ledger.md), so when another page disagrees with it, status.md is the current one.

## The four areas

Each area has one entry page that lists its pages in reading order ([areas.md](areas.md) says what belongs where).

| Area | Entry page | For |
|---|---|---|
| Framework | [framework.md](framework.md) | Building an app's logic and screens on the runtime, ForgeUI, ForgeAppKit and the native services |
| Tools | [tools.md](tools.md) | Getting set up, the `forge` command, testing on the phone |
| Infrastructure | [infrastructure.md](infrastructure.md) | Builds and the build queue, signing, toolchain versions, releases |
| Apps | [examples/README.md](../examples/README.md) | Every app, with its README, acceptance record and design docs |

## Status and plans

| Page | What it's for |
|---|---|
| [status.md](status.md) | What works today, in one page (generated) |
| [capability-ledger.md](capability-ledger.md) | Every capability with its evidence (generated) |
| [app-parity.md](app-parity.md) | Which apps keep up with the framework, and when the rest catch up (generated) |
| [roadmap.md](roadmap.md) | What comes next, and what isn't planned |
| [areas.md](areas.md) | The four areas, what belongs to each, and which way dependencies may run |

## Background

[research/](research/) holds open investigations (the reinvention audit, the accessibility audit, App Intents on Linux, the app friction log). [archive/](archive/README.md) lists retired pages, each linked to its last version in git history.
