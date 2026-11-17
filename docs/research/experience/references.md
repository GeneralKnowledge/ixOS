# Reference catalogue

Status: 2026-11-17. Part of [ixOS Experience Research](README.md). Which mature apps and projects we study, for which ixOS app, and what we want to learn from each. Nothing here is a dependency.

The rule for every entry: **reverse-engineer the decisions, don't copy the appearance.** Why is an action prominent, why is something hidden, how does the app say it is unsure, what happens after a mistake. Several references are AGPL (Signal, Ice Cubes): read them for behaviour and structure, and copy no code. LocalSend is Flutter, so it is a workflow reference, not a SwiftUI one.

## Real applications

These matter more than component libraries: they have solved hard UX problems in real use.

| ixOS app | Reference | Stack | What to investigate | Deep audit |
|---|---|---|---|---|
| Relay | [LocalSend](https://github.com/localsend/localsend) | Flutter (Dart), Apache-2.0 | Send and Receive as the top-level choice; nearby discovery without pairing; how the receiver accepts or declines; per-file progress; trust (fingerprints, favourites); failure wording; what happens when the app is backgrounded | **1st**: [done](references/localsend.md) |
| Relay | [iroh-drop](https://github.com/chad/iroh-drop), [RelayPonyCore](https://github.com/norsehorse-dev/RelayPonyCore), [Drift](https://github.com/vsamarth/drift), [Daak](https://github.com/darshitpp/daak) | Rust on iroh, Swift, Flutter, React Native; MIT or Apache-2.0 | Consent before bytes land; catching up after being apart; retry on presence; honest broadcast wording; wire fixtures | **4th**: [done](references/relay-transfer-peers.md) |
| Overall design language | [Ice Cubes](https://github.com/Dimillian/IceCubesApp) | SwiftUI, AGPL-3.0 | Navigation in a large SwiftUI app; contextual actions on dense rows; customisation without clutter; loading and pagination of feeds; complex information in a list; its per-feature package split | **2nd**: [done](references/ice-cubes.md) |
| Commonplace (and Pocket Library's reader) | [NetNewsWire](https://github.com/Ranchero-Software/NetNewsWire) | UIKit/AppKit, MIT | Next unread; list density; Mark All as Read with undo; feed discovery; organisation (folders, smart feeds); sync state. Commonplace was already benchmarked on behaviour (#148); this pass is about the experience | **3rd**: [done](references/netnewswire.md) |
| Chat | [Signal for iOS](https://github.com/signalapp/Signal-iOS) | UIKit, AGPL-3.0 | First run with nothing technical; composing, attachments and voice notes; message states and delivery marks; permission prompts in context; long-press actions and reactions; recovering from a failed send | Later |
| Weather | Open-source iOS weather apps (to shortlist from an open-source iOS app list) and, for behaviour only, Apple Weather and national weather services | Various | Forecast hierarchy, hourly and daily presentation, maps, warnings, glanceable "should I take a coat" information. Goal: Weather's own idea, not a closer copy of Apple's | Later |
| Soundscape | Apple Music and dedicated audio players (behaviour only) | Closed | Playback controls, library browsing, artwork, queue, Now Playing, interruptions | Later, after the #269 series |
| Pocket Library | Apple Books (behaviour only); NetNewsWire for the reading view | Closed / MIT | Reading Now first; continue in one tap; reading view comfort; organisation and goals | Later |
| Device Lab | Apple's developer tools (Instruments, Console, Settings > Developer) | Closed | Technical density, progressive disclosure, which information is first | Later |
| Good Dog, Checklist, FieldKit, Sketchbook | Not yet chosen (candidates: coaching apps for Good Dog; Reminders and Things for Checklist; outdoor recorders for FieldKit; Notes and Freeform drawing for Sketchbook) | | | Decide in phase 2 |

Note on Pocket Library: NetNewsWire is a feed reader, so its closest ixOS match is Commonplace. The proposal is NetNewsWire for Commonplace and for the shared reading view (ForgeReader), and Apple Books for Pocket Library's library and "continue reading" flow. That is decision 5 in the [audit](audit.md#decisions-for-reuben).

## Design systems

Study for API shape, documentation and accessibility practice. ixOS needs its own identity, so none is adopted wholesale. Licences as GitHub reports them on 2026-11-17.

| Project | What it offers ixOS | Status |
|---|---|---|
| [DaVinci](https://github.com/frankgumeta/DaVinci) (MIT) | Tokens, themes, dark mode and accessibility in a modular SwiftUI design system | Already studied for ForgeUI 0.1 ([forgeui-proposal.md](https://github.com/GeneralKnowledge/ixOS/blob/8fdd497cc51dfd0a8220f7dc013c3a25515f6c51/docs/research/forgeui-proposal.md)): palette values resolved from the colour scheme and the contrast validator came from it. Revisit only for motion and type roles |
| [Orange Unified Design System for iOS](https://github.com/Orange-OpenSource/ouds-ios) (MIT) | A serious SwiftUI component library: component API naming, per-component accessibility notes, documentation structure ([DocC documentation](https://github.com/Orange-OpenSource/ouds-ios-documentation)) | Study in phase 2 for how each component documents its accessibility and its tokens |
| [OUDS Design System Toolbox](https://github.com/Orange-OpenSource/ouds-ios-design-system-toolbox) (MIT) | OUDS's separate showcase app, with code examples per component | Study next to the [ForgeUI showcase](../../../examples/forgeui-showcase/README.md): how examples and documentation sit together |
| [Stance](https://github.com/BindsHQ/stance) (Apache-2.0) | Tokens shared across web, iOS and Android while keeping native components | An architectural reference, not a mature benchmark: its native iOS package is still in development. Relevant when Android resumes; for now a check that ForgeUI's tokens (plain Foundation values) could be exported the same way |
| [Mira](https://github.com/Dtheme/Mira) (MIT) | A SwiftUI design-system playground: styles, visual tokens, interactive demos and implementation guidance | Relevant to agent-assisted work: study in phase 2 with the design-spec idea below |

## Libraries

Swift packages proposed on 2026-11-17, checked against ixOS's rules: Rust owns state, the apps build with xtool on Linux and the phone is the test device, and a dependency is added only for a real second use. Licences and activity as GitHub reported them that day.

| Library | What it does | Verdict for ixOS |
|---|---|---|
| [Nuke](https://github.com/kean/Nuke) (MIT) | Image loading, caching and prefetching | **Not adopted (Reuben, 2026-11-17).** ixOS has no third-party Swift packages, and `ForgeImages` already decodes files at the size shown with a memory cache. Pictures from the web (Pocket Library covers, Soundscape artwork, Commonplace article images, all on plain `AsyncImage`) get the same treatment by extending `ForgeImages` with a disk-cached download, in its own PR. Revisit Nuke only if prefetching, progressive or animated images become a real need |
| [swift-navigation](https://github.com/pointfreeco/swift-navigation) (MIT) | State-driven navigation and presentation | **Idea already borrowed.** ForgeAppKit's destinations (`.forgeSheet`, `.forgeNavigationDestination`, [forgeappkit.md](../../forgeappkit.md)) are its pattern without the dependency or its macros |
| [Pulse](https://github.com/kean/Pulse) (MIT) | Log and URLSession network inspector with a SwiftUI viewer | **Idea only.** Most networking is in Rust and logging goes through `forge_runtime::logging`, so Pulse would see little. Its viewer is a model for a Device Lab diagnostics screen |
| [GRDB.swift](https://github.com/groue/GRDB.swift) (MIT) | SQLite toolkit with observation and migrations | **No.** Persistence belongs to the Rust core (`forge-storage`, rusqlite); a Swift database would split state across the boundary |
| [SwiftUI Introspect](https://github.com/siteline/swiftui-introspect) (MIT) | Reach the UIKit view under a SwiftUI one | **Escape hatch, per app.** Never in ForgeUI, which styles real SwiftUI controls |
| [ScanCodes](https://github.com/danielsaidi/ScanCodes) (MIT) | QR and barcode generation and scanning | **No.** Relay already makes QR codes with Core Image; VisionKit scans |
| [OpenSwiftUI](https://github.com/OpenSwiftUIProject/OpenSwiftUI) (MIT) | Open-source reimplementation of SwiftUI | **Reading only**, for how SwiftUI behaves |
| SnapshotTesting, AccessibilitySnapshot | See below | Simulator only; their ideas are listed under quality tools |

## Tools for keeping quality from slipping

| Project | What it gives | Fit for ixOS today | Proposal |
|---|---|---|---|
| [SnapshotTesting](https://github.com/pointfreeco/swift-snapshot-testing) | Screenshot regression tests for views and screens | Needs XCTest on an iOS simulator, so macOS only; ixOS is Linux first and CI gates on Linux. ixOS already has phone baselines with a tolerant pixel diff ([visual-regression.md](../../visual-regression.md)) | Borrow its ideas (named variants, one baseline per configuration), already partly done. Adopt only if macOS CI becomes routine |
| [AccessibilitySnapshot](https://github.com/cashapp/AccessibilitySnapshot) | Regression tests for the accessibility hierarchy: what VoiceOver reads, in what order | Also simulator-based. But `forge ui run` already saves the accessibility tree (`name.tree.json`) next to every screenshot | Compare the saved trees like the pixels: a lost label, a changed order or a missing identifier fails the run. Small work in forge-uitest, but `forge ui` is frozen to bug fixes (AGENTS.md rule 11), so this needs your go-ahead |
| Apple's accessibility audit | Contrast, element descriptions, Dynamic Type issues, from the device | The [ax spike](../ax-audit.md) reads the screen and presses controls; the audit itself isn't working yet | Finish the audit call (one known probe bug) as part of phase 3's checks |
| [Awesome iOS Design MD](https://github.com/Meliwat/awesome-ios-design-md) (MIT; about 200 documented app design systems as DESIGN.md files) | Precise design specifications to hand to an agent instead of "make it modern" | Fits the plan: phase 2 writes ixOS's own design spec in the same shape | Read a handful in phase 2 to choose the spec's format |

## Deep audit template

Each deep audit (one PR per reference) records, for every significant issue:

| Field | What |
|---|---|
| Current behaviour | What the ixOS app does today, with a screenshot or the source line |
| Reference behaviour | What the reference does differently, with where in its code or UI |
| Why it is better | The decision behind it, in one or two sentences |
| Where it belongs | App, ForgeUI, ForgeAppKit, a check, or docs |
| Proposal and priority | The change, P1 to P3 |

Plus a short "what the reference gets wrong" section, so we don't import its mistakes.

Reference screenshots go in the repo only when we are entitled to reproduce them (our own apps, or a reference's images under a licence that allows it, with attribution). Otherwise a deep audit links to the source and describes the observation in words; annotated links are usually enough.
