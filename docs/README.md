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
| [app-plan.md](app-plan.md) | What each app builds next, and the framework parts those features need |
| [areas.md](areas.md) | The four areas, what belongs to each, and which way dependencies may run |
| [audits/half-baked-work-audit.md](audits/half-baked-work-audit.md) | Unfinished and half-baked work in the foundation: what is unproven, duplicated or misleading, and the order to fix it |
| [audits/defect-hunt-2026-11.md](audits/defect-hunt-2026-11.md) | Bugs a phone sitting misses (November 2026): seven ranked defects with evidence, the same flaws checked across every app, and the fix PRs to pick from |
| [audits/structure-audit.md](audits/structure-audit.md) | The `forge` CLI, area boundaries and Device Lab: parallel systems and drift between lists, with the order to fix them |
| [design/README.md](design/README.md) | The design programme for Soundscape and Relay: philosophy, reference study, tokens, both apps' specs, design QA and the PR roadmap |

## Background

[research/](research/) holds open investigations (the reinvention audit, the accessibility audit, App Intents on Linux, the app friction log, the [everyday capabilities audit and plan](research/everyday-capabilities.md), the [app capability audit for A2UI](research/app-capability-audit.md) (the typed actions and data each app could safely expose to an agent-composed surface), the [eSIM and AI Lab focus plan](research/esim-ai-lab-plan.md), the [better-by-default plan](research/better-by-default.md), the [framework candidates survey](research/framework-candidates.md), the [one native-services route design note](research/one-native-services-route.md), the [app-as-specification plan](research/app-as-specification.md) (rebuild a small app from its accessibility tree and compare the two), [ixOS Experience Research](research/experience/README.md): the UX audit, the reference catalogue and the [ForgeUI coverage audit](research/experience/forgeui-coverage.md) and the [app UI review](research/experience/app-ui-review-2026-11.md), the [ForgeUI 0.2 audit and specification](research/experience/forgeui-0.2.md), the [UI Finish Audit](research/experience/ui-finish-audit.md), the [App Studio specification and forge-lab audit](research/app-studio.md) (a proposal: describe an app, an agent builds it as a Forge project), the [business-to-app specification](research/business-to-app.md) (a proposal: a local business's website becomes a verified profile and a native preview in Device Lab, as a sales opener), the [open-source Android ports plan](research/android-ports.md) (Commonplace answering Feeder, FieldKit answering OpenTracks, Soundscape answering AntennaPod), the [on-device AI for Android findings and comparison plan](research/android-local-ai.md) (the stage after Android's A8), the [unsafe Rust audit and security-logic verification plan](research/unsafe-and-verification.md), and the [External Projects Atlas](research/external-projects/README.md): which outside projects were researched, borrowed from or adopted, and which are candidates). [archive/](archive/README.md) lists retired pages, each linked to its last version in git history.
