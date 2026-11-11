# Forge docs

Forge builds native iPhone apps from Linux. You write the screens in SwiftUI and the logic in Rust, and Forge builds the app, signs it and installs it on your iPhone. ixOS is the name of the repository; Forge is the tool it gives you. No Mac is needed.

**Does it work yet?** [status.md](status.md) lists what works on an iPhone today, what is built but not yet proven on a phone, and what isn't built. It is generated from the [capability ledger](capability-ledger.md), so when another page's status note disagrees with it, status.md is the current one.

## Start here

Read these in order the first time.

| Page | What it's for |
|---|---|
| [getting-started.md](getting-started.md) | From a bare Linux machine to your first app on your iPhone, about an hour |
| [first-app.md](first-app.md) | Add a feature to the minimal starter app (Rust rule, test, view model, button), about 20 minutes |
| [human-workflow.md](human-workflow.md) | The whole loop with the CLI alone, from `forge init` to reviewing evidence: no AI agent, MCP server or API key at any step |
| [troubleshooting.md](troubleshooting.md) | A build, install or launch failed, or the app opens to a blank screen (`forge diagnose` checks it for you) |
| [linux-setup.md](linux-setup.md) | Each toolchain step by hand, if `forge setup ios` couldn't do one |

## Building an app

| Page | What it's for |
|---|---|
| [app-crate.md](app-crate.md) | Your app's Rust crate: state, rules, saving, networking, events |
| [ios-ui.md](ios-ui.md) | The SwiftUI side: how a Forge app should look and how to show Rust state |
| [forgeui.md](forgeui.md) | ForgeUI, the design system: theme tokens, button and field styles, rows, cards, badges, states; usable in any SwiftUI app |
| [app-foundations.md](app-foundations.md) | Rust application foundations in `forge-app`: saved state as one versioned document with recovery rules, field-by-field validation |
| [persistence.md](persistence.md) | What Forge promises about saved data at each crash point, and what an app does: stage a file before the record that names it, sweep at launch, keep a fixture per schema version |
| [forgeappkit.md](forgeappkit.md) | ForgeAppKit, application patterns on ForgeUI: alerts, confirmations and destinations as model values, load and submission state, form errors |
| [architecture.md](architecture.md) | How the Rust core, the Swift layer and the generated bindings fit together |
| [architecture-assessment.md](architecture-assessment.md) | The architecture's trade-offs: what is intentional, what is a current limitation, and what to improve next, in order |
| [ffi-policy.md](ffi-policy.md) | The rules for calls between Swift and Rust: async, events, cancellation, errors |
| [native-services.md](native-services.md) | Keychain, notifications, permissions, location and other iPhone features |
| [native-capability-roadmap.md](native-capability-roadmap.md) | Which Apple frameworks ixOS wraps next (web view, reader, documents, media, background tasks, identity, maps, charts), in what order, and how each is proven |
| [app-config.md](app-config.md) | `forge.toml`: name, version, icon, permission prompts, entitlements |
| [project-generation.md](project-generation.md) | What `forge init` creates, with and without `--minimal` |

## Signing, devices and builds

| Page | What it's for |
|---|---|
| [ios-signing.md](ios-signing.md) | Apple sign-in, free and paid teams, the 7-day limit and the 3-app limit |
| [builds.md](builds.md) | What each kind of build produces, and what isn't supported yet |
| [first-app-store-release.md](first-app-store-release.md) | The plan for publishing to the App Store (not supported yet) |
| [ios-xcode.md](ios-xcode.md) | Building with Xcode on a Mac, if you have one (optional) |
| [compatibility.md](compatibility.md) | The tool versions Forge is built and tested with |
| [android-setup.md](android-setup.md) | Android, which is work in progress |

## Testing and fixing an app on the phone

| Page | What it's for |
|---|---|
| [forge-device.md](forge-device.md) | Your iPhone from Linux: details, live logs, crash reports |
| [forge-ui-testing.md](forge-ui-testing.md) | UI tests that tap and type on a real iPhone (`forge ui`) |
| [forge-explore.md](forge-explore.md) | Walking an app's screens without a script to find problems (`forge ui explore`) |
| [benchmarks/](benchmarks/README.md) | Replication benchmarks: rebuilding a real app (NetNewsWire, as Commonplace) to find what ixOS is missing; gap logs and pattern inventories |
| [visual-regression.md](visual-regression.md) | Noticing when a change made a screen look worse |
| [evidence.md](evidence.md) | Recording what a run proved, so a feature is shown to work rather than said to |

## For coding agents (optional)

Nothing on the pages above needs an agent. Agents use the same commands; these pages add the optional integration (`forge agent setup`, `forge mcp`).

| Page | What it's for |
|---|---|
| [AGENTS.md](../AGENTS.md) | The rules an agent follows in this repository |
| [forge-mcp.md](forge-mcp.md) | `forge mcp`: Forge as tools an AI agent can call |
| [dev-loop.md](dev-loop.md) | Each step of the edit, build, install, test and fix loop, and how far it is proven |

## Status and plans

| Page | What it's for |
|---|---|
| [status.md](status.md) | What works today, in one page (generated) |
| [capability-ledger.md](capability-ledger.md) | Every capability with its evidence, column by column (generated) |
| [roadmap.md](roadmap.md) | What comes next, and what isn't planned |

## Background

These pages are for people working on Forge itself. They are plans, reviews and records of past work, and their status notes describe the day they were written. Check [status.md](status.md) for where things stand now.

- Demo designs: [chat-demo.md](chat-demo.md), [good-dog-demo.md](good-dog-demo.md), [local-ai.md](local-ai.md), [bevy-game-trial.md](bevy-game-trial.md)
- Milestone plans and reports: [forge-m6.md](forge-m6.md), [m65-run-report.md](m65-run-report.md), [good-dog-pain-points.md](good-dog-pain-points.md)
- Reviews and audits: [app-model-review.md](app-model-review.md) (what an ixOS application is, and the proposed canonical model), [foundation-review.md](foundation-review.md), [framework-extraction-audit.md](framework-extraction-audit.md), [research/reinvention-audit.md](research/reinvention-audit.md) (where ixOS reinvents existing tools), [research/](research/)
- Phone runbooks and session records: [native-services-device-check.md](native-services-device-check.md), [local-ai-check.md](local-ai-check.md), [integration/](integration/), [testing/](testing/), [toolchain-validation.md](toolchain-validation.md)
- Decisions and history: [adr/](adr/), [ideal-design.md](ideal-design.md) (historical), [release-process.md](release-process.md) (releasing Forge itself, not your app)
