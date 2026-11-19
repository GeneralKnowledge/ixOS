# Prior research register

Status: 2026-11-17, checked against main at `98a8446` and the PRs linked. Part of the [External Projects Atlas](README.md).

What ixOS has already investigated, where the work is, and what was decided. **Don't investigate these from scratch.** An entry is reopened only when a real requirement changes, and the reopening names it.

State is one of the four in the [README](README.md#four-different-states) (known, researched, borrowed, adopted). Disposition is the decision, using the outcomes in the [evaluation framework](evaluation-framework.md#five-outcomes) where one was made; "undecided" means it was looked at and nothing was decided.

## Libraries and packages

| Project | Where it was looked at | What we already know | State | Disposition |
|---|---|---|---|---|
| [swift-navigation](https://github.com/pointfreeco/swift-navigation) (MIT) | [references.md, Libraries](../experience/references.md#libraries) (#297, 2026-11-17) | ForgeAppKit's destinations (`.forgeSheet`, `.forgeNavigationDestination`) are its state-driven pattern, without the package or its macros | Borrowed | **Borrow.** Keep using the pattern; don't add the package |
| [Nuke](https://github.com/kean/Nuke) (MIT) | [references.md, Libraries](../experience/references.md#libraries) (#297) | A good library, not rejected on quality. ixOS has no third-party Swift packages, and ForgeImages already decodes at the size shown with a memory cache | Researched | **Superseded by an implementation decision (Reuben, 2026-11-17):** extend ForgeImages with a disk-cached download instead, PR #298, merged 2026-11-17. Revisit only if prefetching, progressive or animated images become a real need |
| [Pulse](https://github.com/kean/Pulse) (MIT) | [references.md, Libraries](../experience/references.md#libraries) (#297) | Most networking is in Rust and logging goes through `forge_runtime::logging`, so a URLSession inspector would see little | Researched | **Idea only:** its viewer is a model for a Device Lab diagnostics screen |
| [GRDB.swift](https://github.com/groue/GRDB.swift) (MIT) | [references.md, Libraries](../experience/references.md#libraries) (#297) | Persistence belongs to the Rust core (`forge-storage`, rusqlite); a Swift database would split state across the boundary | Researched | **Reject** |
| [SwiftUI Introspect](https://github.com/siteline/swiftui-introspect) (MIT) | [references.md, Libraries](../experience/references.md#libraries) (#297) | Reaches the UIKit view under a SwiftUI one | Researched | **Escape hatch, per app only.** Never in ForgeUI |
| [ScanCodes](https://github.com/danielsaidi/ScanCodes) (MIT) | [references.md, Libraries](../experience/references.md#libraries) (#297) | Relay already makes QR codes with Core Image; VisionKit scans | Researched | **Reject:** native APIs preferred |
| [OpenSwiftUI](https://github.com/OpenSwiftUIProject/OpenSwiftUI) (MIT) | [references.md, Libraries](../experience/references.md#libraries) (#297) | An open reimplementation of SwiftUI | Researched | **Study:** reading only, for how SwiftUI behaves |
| [SnapshotTesting](https://github.com/pointfreeco/swift-snapshot-testing) (MIT) | [references.md, quality tools](../experience/references.md#tools-for-keeping-quality-from-slipping) (#285) | Needs XCTest on an iOS simulator, so macOS only; ixOS has phone baselines with a tolerant pixel diff ([visual-regression.md](../../visual-regression.md)) | Researched | **Borrow ideas** (named variants, one baseline per configuration); reassess only if macOS CI becomes routine |
| [AccessibilitySnapshot](https://github.com/cashapp/AccessibilitySnapshot) (Apache-2.0) | [references.md, quality tools](../experience/references.md#tools-for-keeping-quality-from-slipping) (#285) | Simulator-based too. `forge ui run` already saves `name.tree.json` beside every screenshot | Researched | **Borrow idea, undecided:** diff the saved trees like the pixels; needs Reuben's go-ahead because `forge ui` is frozen. Reassess next to the a11y-check assessment |
| [CoreML-LLM](https://github.com/john-rocky/CoreML-LLM) (MIT) | [local-ai.md](../../local-ai.md) (approved 2026-11-07) | Qwen and Gemma on the Neural Engine, models downloaded and compiled on the phone; iOS 18; Qwen3.5 runs through a callback API without built-in cancellation | Researched | **Chosen route for downloadable models (L2)**, subject to its xtool build spike |
| [MLX Swift](https://github.com/ml-explore/mlx-swift) (MIT) | [local-ai.md](../../local-ai.md) (2026-11-07) | MLX's own README says SwiftPM cannot build its Metal shaders; the final build needs Xcode | Researched | **Poor fit for Linux.** The atlas's P1 spike must start from this finding, not from scratch |
| [llama.cpp](https://github.com/ggml-org/llama.cpp) (MIT) | [local-ai.md](../../local-ai.md), [forge-ai-plan.md](../../forge-ai-plan.md) | Prebuilt XCFramework as a binary target, so nothing compiles on Linux; untested with xtool. Also the laptop-side server for AI over Relay | Researched | **Fallback** iOS adapter if CoreML-LLM fails; laptop upstream for `relay serve-ai` |
| Apple Foundation Models | [local-ai.md](../../local-ai.md) | On-device model, iOS 26, Apple Intelligence must be on. L1 passed on the phone | Adopted | **Adopt** (Chat's Assistant, now in Relay) |
| [a11y-check](https://github.com/cvs-health/ios-swiftui-accessibility-techniques) (Apache-2.0) | [assessment](assessments/accessibility/a11y-check.md) (2026-11-17, `3aa074c`) | SwiftSyntax source checker, 47 WCAG rules; builds and passes 340 tests on Linux in `swift:6.2`; real findings in ixOS apps, but several frequent rules are noisy or blind to `#if` | Researched | **Adopt as a CI tool** (recommendation; not added yet), report-only, gating on chosen rules with a baseline |
| [FluidAudio](https://github.com/FluidInference/FluidAudio) (Apache-2.0; models licensed separately) | [assessment](assessments/ai/fluidaudio.md) (2026-11-17, `16780f0`) | Core ML speech models downloaded precompiled; the binary NeMo target is an opt-out trait. Not built with xtool yet | Researched | **Watch:** Apple's on-device speech recognition first |
| [re-appintentsmetadataprocessor](https://codeberg.org/viraptor/re-appintentsmetadataprocessor) (MIT) | [app-intents-on-linux.md](../app-intents-on-linux.md) (2026-11-11) | Rust reimplementation of Xcode's App Intents metadata step, which xtool lacks; English-only App Shortcuts | Researched | **Undecided:** two checks (laptop build, one intent in Shortcuts on the phone) before any Forge code |

## Relay and networking

| Project | Where it was looked at | What we already know | State | Disposition |
|---|---|---|---|---|
| [iroh](https://github.com/n0-computer/iroh), iroh-blobs (MIT or Apache-2.0) | [relay-transfer-plan.md](../../../examples/relay/docs/relay-transfer-plan.md) (#197, 2026-11-14), M0 issue #198, [relay-stage0-spike.md](../../../examples/relay/docs/relay-stage0-spike.md) | Chosen transport. M0 passed on two Linux machines (#199); the iPhone half passed at the start of M1 (laptop to iPhone 1 GB, iPhone to Linux). Production transfer core merged in #267 | **Adopted** | **Adopt**, in Relay (`examples/relay/rust/Cargo.toml`: `iroh` 1, `iroh-blobs` =0.104.1). No longer a feasibility track: further work is Relay's own (outbox #299, trusted devices) |
| spake2 (RustCrypto, MIT or Apache-2.0) | [relay-transfer-plan.md](../../../examples/relay/docs/relay-transfer-plan.md) | Word codes with key confirmation | Adopted | **Adopt**, in Relay |
| [LocalSend](https://github.com/localsend/localsend) (Apache-2.0, Flutter) | [relay-transfer-plan.md](../../../examples/relay/docs/relay-transfer-plan.md) (as a base); [deep audit](../experience/references/localsend.md) (#288) | Rejected as Relay's protocol (multicast, PIN auth). Its Send/Receive front door, nearby targets and help sheet shaped Relay's journey | Researched | **Study:** existing workflow reference, not a protocol |
| [iroh-drop](https://github.com/chad/iroh-drop) (MIT or Apache-2.0) | [relay-transfer-peers.md](../experience/references/relay-transfer-peers.md) (#297, commit `62d460f`) | Same iroh stack. Accept-before-bytes, catch-up on neighbour-up, honest LAN-broadcast wording | Researched | **Borrow** behaviour (now in #299's outbox and the transfer journey) |
| [Drift](https://github.com/vsamarth/drift) (MIT) | [relay-transfer-peers.md](../experience/references/relay-transfer-peers.md) (#297, `f840ce5`) | Rust on iroh 0.97, Flutter, no iOS app yet | Researched | **Study:** reference; iOS feasibility unknown |
| [Daak](https://github.com/darshitpp/daak) (Apache-2.0) | [relay-transfer-peers.md](../experience/references/relay-transfer-peers.md) (#297, `de23b51`) | Rust core over UniFFI like ixOS; backoff reset on presence. Self-described "fully vibecoded", two stars: trust none of its protocol | Researched | **Study:** experimental reference |
| [RelayPonyCore](https://github.com/norsehorse-dev/RelayPonyCore) (Apache-2.0) | [relay-transfer-peers.md](../experience/references/relay-transfer-peers.md) (#297, `72d4f8f`) | Open core of a shipping iPhone app: age over Bonjour and Network.framework; the app itself is closed | Researched | **Study:** UX and implementation reference |
| [Lightning P2P](https://github.com/Kerim-Sabic/lightning-p2p) (Apache-2.0) | [assessment](assessments/networking/lightning-p2p.md) (2026-11-17, `ce2ee63`) | Same iroh stack; bearer tickets; no iOS app. Its retention code led to measuring that Relay keeps received files twice | Researched | **Borrow** (retention, request cap, relay wait, receive journal) |
| [libp2p](https://github.com/libp2p/rust-libp2p) | [relay-transfer-plan.md](../../../examples/relay/docs/relay-transfer-plan.md) | Does what iroh does with more moving parts and a weaker iOS story | Researched | **Reject** as Relay's transport |
| [Syncthing](https://github.com/syncthing/syncthing) (MPL-2.0) | [relay-transfer-plan.md](../../../examples/relay/docs/relay-transfer-plan.md) | Continuous folder sync is a different product; a later "keep this folder in sync" mode could borrow its ideas | Researched | **Watch** for a sync mode |
| croc, magic-wormhole, Floe, wush, derphole, WebRTC, BitTorrent/IPFS | [relay-transfer-plan.md](../../../examples/relay/docs/relay-transfer-plan.md) | croc: borrow the flow. magic-wormhole: borrow SPAKE2 word codes, not the protocol (internet rendezvous, EUPL crate). Floe: browser receiver idea for stage 3. wush/derphole: ride Tailscale's DERP. Torrent/IPFS: built for one-to-many | Researched | **Borrow** (croc, magic-wormhole designs); **Watch** (Floe browser receiver); **Reject** as base (the rest) |
| Nostr (Marmot, NIP-29, NIP-11), Matrix, IRC, XMTP, iroh-gossip | [relay-rooms-protocols.md](../../../examples/relay/docs/relay-rooms-protocols.md) (#197) | Rooms speak Nostr, approved by Reuben 2026-11-14 | Researched | **Adopt** Nostr for rooms (not built yet); others compared and not chosen |

## Apps and design systems studied for experience

From [ixOS Experience Research](../experience/README.md) (#285 and its deep audits). These are product benchmarks: behaviour and decisions, never code.

| Project | Where it was looked at | Disposition |
|---|---|---|
| LocalSend | Deep audit 1, [localsend.md](../experience/references/localsend.md) (#288) | Existing reference for Relay |
| [Ice Cubes](https://github.com/Dimillian/IceCubesApp) (AGPL-3.0) | Deep audit 2, [ice-cubes.md](../experience/references/ice-cubes.md) (#291) | Existing product benchmark: text roles, haptics, banner with Undo (ForgeBanner, #294) |
| [NetNewsWire](https://github.com/Ranchero-Software/NetNewsWire) (MIT) | Deep audit 3, [netnewswire.md](../experience/references/netnewswire.md) (#293); earlier behaviour benchmark (#148) | Existing product benchmark for Commonplace and ForgeReader |
| [Signal for iOS](https://github.com/signalapp/Signal-iOS) (AGPL-3.0) | [references.md](../experience/references.md#real-applications) (#285), deep audit marked "Later" | Existing reference for Chat (now in Relay); not yet audited |
| [DaVinci](https://github.com/frankgumeta/DaVinci) (MIT) | ForgeUI 0.1 proposal; [references.md](../experience/references.md#design-systems) | **Borrowed:** palette resolved from the colour scheme and the contrast validator. Revisit only for motion and type roles |
| [OUDS iOS](https://github.com/Orange-OpenSource/ouds-ios), [OUDS Toolbox](https://github.com/Orange-OpenSource/ouds-ios-design-system-toolbox) (MIT) | [references.md](../experience/references.md#design-systems) | Design-system reference: per-component accessibility notes, showcase structure |
| [Stance](https://github.com/BindsHQ/stance) (Apache-2.0) | [references.md](../experience/references.md#design-systems) | Architectural reference for cross-platform tokens, when Android resumes |
| [Mira](https://github.com/Dtheme/Mira) (MIT) | [references.md](../experience/references.md#design-systems) | Design-system reference for agent-assisted design specs |
| [Awesome iOS Design MD](https://github.com/Meliwat/awesome-ios-design-md) (MIT) | [references.md](../experience/references.md#tools-for-keeping-quality-from-slipping) | Format reference for ixOS's own design spec |

## Apps studied for techniques

| Project | Where it was looked at | Disposition |
|---|---|---|
| [Offscan](https://github.com/bartoszpiech/offscan) (MIT) | [assessment](assessments/media/offscan.md) (2026-11-17, `7b8d682`) | **Borrow and Study:** pipeline and recipe design for roadmap item 18; not a dependency (SwiftData state, five days old) |

## Tooling

| Project | Where it was looked at | What we already know | Disposition |
|---|---|---|---|
| [Maestro](https://github.com/mobile-dev-inc/maestro), Appium, Firebase Robo, mobile-mcp | [reinvention-audit.md](../reinvention-audit.md) (2026-11-15) | Maestro does not support physical iPhones; Appium off macOS needs iOS 18+ and a root tunnel | **Reject** for ixOS's physical-iPhone testing today. The atlas lists Maestro again under testing: start from this finding |
| Apple's accessibility audit service (Accessibility Inspector's daemon) | [ax-audit.md](../ax-audit.md) (2026-11-07) | Reading the screen and pressing by identifier work from Linux without the DDI; the audit call itself isn't working yet (one probe bug) | **Adopt in progress** (spike); finish the audit call in the experience programme's phase 3 |
| WebDriverAgent | [forge-ui-testing.md](../../forge-ui-testing.md) | Text entry and UI tests on the physical iPhone | **Adopted** (`forge ui`) |
| [xtool](https://github.com/xtool-org/xtool) | Fork at GeneralKnowledge/xtool | The build toolchain | **Adopted.** Changes go to the fork only |
| [Craft Anything](https://github.com/storytold/craft-anything) (MIT or Apache-2.0) | [app-studio.md](../app-studio.md#external-tools-checked) (2026-11-19) | Study, build, check, refine loop; drives Claude Code as its builder; early alpha; iOS and SwiftUI are not output targets | **Study**, reassess after its first public release. Not a dependency |
| [REA](https://github.com/morluto/rea) (MIT) | [app-studio.md](../app-studio.md#external-tools-checked) (2026-11-19) | Agent reverse-engineering over CLI and MCP (binaries via Hopper or Ghidra, JS/Electron, .NET, IPA/APK); builds nothing | **Watch:** optional reference study, only on software the person may examine |
| [OpenHands CLI](https://github.com/OpenHands/OpenHands-CLI) (MIT) | [app-studio.md](../app-studio.md#external-tools-checked) (2026-11-19) | Headless mode and MCP, but its README says it is no longer actively maintained (Agent Canvas replaces it) | **Watch** Agent Canvas as a possible second agent backend |
| [Aider](https://github.com/Aider-AI/aider) (Apache-2.0) | [app-studio.md](../app-studio.md#external-tools-checked) (2026-11-19) | Scriptable editor (`--message`), local models; no MCP, so it can't use forge-lab | **Reject** as an App Studio backend for now |

## Corrections to the 2026-11-17 recovery summary

Checked against the repository while writing this page:

- **Iroh is adopted, not only an experiment.** #197 and #198 were the feasibility track; the iPhone check passed and the production core merged in #267.
- **MLX Swift and llama.cpp were already assessed** in [local-ai.md](../../local-ai.md) (MLX: poor fit for Linux builds; llama.cpp: workable fallback).
- **Maestro and App Intents were already looked at** ([reinvention-audit.md](../reinvention-audit.md), [app-intents-on-linux.md](../app-intents-on-linux.md)).
- **libp2p and Syncthing were already compared** for Relay and not chosen.
- **iroh-drop** was the fourth project in #297 next to Drift, Daak and RelayPony, and the most relevant one.
- **#298 merged on 2026-11-17** (after the summary): ForgeImages now downloads and disk-caches web pictures, so the Nuke decision is implemented.
