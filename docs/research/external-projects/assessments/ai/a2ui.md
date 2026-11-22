# A2UI: agent-composed native interfaces beside ForgeUI?

Status: 2026-11-22. Part of the [External Projects Atlas](../../README.md). Research only: nothing is added to ixOS by this page. This is milestone A2UI-0 of Reuben's A2UI plan (2026-11-22); it ends with a proposed A2UI-1 scope that waits for his approval.

## Question

Can A2UI give ixOS apps a safe way for an AI agent to ask for structured, native interface (a reading plan, a research brief, a review before changes are applied) instead of a block of text or a hand-built screen per interaction, without replacing ForgeUI, ForgeAppKit or the Rust-owns-state architecture? Concretely: does its iOS renderer build with xtool on Linux, does its security model match "generated UI only calls explicitly registered app functions", and which ixOS agent could drive it?

## Evidence

| Field | Value |
|---|---|
| Date | 2026-11-22 |
| Inspected | [a2ui-project/a2ui](https://github.com/a2ui-project/a2ui) (`google/A2UI` serves the same tree) at `5ff1b05` (main, 2026-11-20). Swift release tags `v0.10.0+swift`, `v0.11.0+swift`; `swift/README.md` says to depend on `0.11.0` |
| Licence | Apache-2.0 (`LICENSE`; every Swift file carries the Apache header). Dependencies: swift-json-schema (MIT), swift-collections (Apache-2.0), Yams (MIT, tests only) |
| Activity | Very active: many packages released through 2026 (Python core 0.1 to 0.7, agent SDK 0.6 to 0.12), commits up to the day before this inspection, a large community renderer list (`docs/site/src/content/docs/ecosystem/renderers.mdx`) |
| Read or run | Protocol v1.0 spec (`specification/v1_0/docs/`), Swift core and SwiftUI adapter sources and manifest, SDK and renderer pages, the 1.0 announcement. **Not built** with xtool, **not run** on the iPhone |
| Build evidence | Not built. Findings below come from `Package.swift` and the sources |
| Platform evidence | Not run. The Swift renderer targets iOS 17+ and macOS 14+, the same minimum as ForgeKit and the apps (`forgekit/Package.swift`, `examples/device-lab/Package.swift`) |
| ixOS baseline | main at `fd095d7`. ForgeUI components in `forgekit/Sources/ForgeUI/Components/`; AI request types in `crates/forge-services/src/ai/mod.rs`; the Bonsai Harness's grammar-constrained calls in `examples/device-lab/rust/src/harness/calls.rs`; per-app plan in [app-plan.md](../../../../app-plan.md) |

## Reuben's claims, checked against primary sources

| Claim | Result | Source |
|---|---|---|
| A2UI 1.0 announced 11 November 2026 | **Confirmed.** The post is dated Nov. 11, 2026 and calls 1.0 "the first stable version of our open protocol for generative UI" | [Google Developers Blog](https://developers.googleblog.com/a2ui-10-build-delightful-fast-reliable-agentic-experiences/) |
| Jetpack Compose renderer is on v0.9.1, with 1.0 coming | **Confirmed.** The post says Compose is "currently v0.9.1, and v1.0 coming soon"; the official SDK table lists `androidx.a2ui.compose` and `material3-a2ui` at v0.9.1 only. No date for Compose 1.0 on the roadmap page | Same post; `docs/site/src/content/docs/concepts/sdks.mdx`; [a2ui.org/roadmap](https://a2ui.org/roadmap/) |
| Apache 2.0 | **Confirmed** for the repository. The blog post itself names no licence; the repository's `LICENSE` does | `LICENSE` at `5ff1b05` |
| Bidirectional function calls, accessibility metadata, incremental updates | **Confirmed**, all new or extended in v1.0 (`callRendererFunction`, `callAgentFunction` and their responses; `@accessibility` with label, description, live, hidden; `updateComponents` and `updateDataModel` by id and JSON Pointer) | `specification/v1_0/docs/a2ui_protocol.md` |
| iOS renders with SwiftUI | **Confirmed, and it is official** (not only community): `A2UISwiftUI` and `BasicCatalogSwiftUI`, v0.9.1 and v1.0 | `Package.swift`, `sdks.mdx` |

## What it does

A2UI is three layers ([README](https://github.com/a2ui-project/a2ui/blob/main/README.md)):

1. **Inference.** A model writes UI in a compact format (A2UI Express, or direct JSON). Agent SDKs (TypeScript, Python, Dart only) turn the model's catalog into a prompt and compile its answer into wire messages. A deterministic program can also write the wire messages itself, with no model; the docs say so explicitly (`concepts/inference-formats.mdx`).
2. **Transport.** A stream of JSON messages from agent to renderer: `createSurface` (v1.0 may carry the whole first tree and data model), `updateComponents`, `updateDataModel`, `deleteSurface`, plus the v1.0 function-call messages. The transport is not defined by A2UI (bindings exist for A2A, AG-UI and MCP; anything that carries JSON works).
3. **Rendering.** A core library holds each surface's component graph and its JSON data model; a framework adapter maps nodes to native views; a **catalog** (a JSON Schema) lists the components and functions an agent may use. A UI is a flat list of components referring to each other by id, bound to the data model with `{"@path": "/plan/books"}`.

What makes it a security boundary, from the v1.0 spec:

- **UI is data.** No code crosses the wire. A component or function not in the registered catalog fails validation (strict by default).
- **Functions are catalog-declared** with `allowedCallers`: `rendererOnly` (the default: only bindings, checks and actions on screen can call it), `agentOnly`, or `rendererOrAgent`. A renderer rejects an agent's call to anything else with `INVALID_FUNCTION_CALL`.
- **`requiresUserActivation: true`** functions run only from a real tap, never from a binding or an agent; the schema forces them to `rendererOnly`. `openUrl` is one, and the Swift catalog opens nothing unless the app supplies a handler.
- **Buttons either send an event** (`action.event` with a name and a context object) **to the agent, or call a local catalog function.** Nothing else happens on a tap.
- **v1.0 removed themes from the protocol**: styling is entirely the native framework's, so an agent can't restyle the app.

### The Swift renderer, as it matters for xtool on Linux

| Item | Why it matters | Assessment |
|---|---|---|
| About 26,000 lines of Swift across `A2UIJSON`, `A2UICore`, `BasicCatalog`, `A2UISwiftUI`, `BasicCatalogSwiftUI` | A large dependency for the size of the first use | Acceptable for a spike; the reason to keep the first use small |
| Dependencies `swift-json-schema` 0.13 (MIT) and through it `swift-collections` | Would be the **first third-party Swift package in ixOS**: ForgeKit and every app depend only on each other today | A real decision, not a detail (see "Open decisions") |
| `swift-json-schema` also declares a macro target on `swift-syntax` | SwiftPM resolves and downloads swift-syntax even though A2UI only uses the `JSONSchema` and `OrderedJSON` products, which don't depend on it | Probably only a slower first resolve; must be checked in the build spike |
| A **SwiftPM build-tool plugin** (`A2UISchemaGeneratorPlugin`) generates code for `A2UIJSON` and `BasicCatalog` | Plugins run on the build machine. ixOS has never built a package with a build-tool plugin under xtool | **The main unknown.** If it fails, the fallback is to vendor the generated files (which the Apache licence allows, with notices) |
| `A2UICore` imports **Combine** in six files (`SurfaceViewModel`, `DataModel`, `NodeResolver`, ...) | Combine doesn't exist on Linux, so the core can't be unit-tested on Linux, unlike ixOS view models (AGENTS.md rule 3) | The A2UI types stay in view-side Swift; anything ixOS wants to test on Linux lives in Rust |
| Resources (`.process("Resources")`, a `.stringsdict`), `AVKit` for the Video component | Resource processing differs between Xcode and SwiftPM | Unchecked; the ixOS catalog would leave Video and AudioPlayer out anyway |
| Accessibility: `@accessibility` mapped to label, hint, hidden and live-region announcements; buttons get `accessibilityIdentifier("a2ui-<id>")` | ixOS rule 4 wants `screen.element` identifiers | Custom ixOS components can set `a2ui.<surface>.<id>`; the basic catalog's own scheme differs |
| Custom components: `ComponentImplementation(api:) { node in ... }` registered in a `Catalog` passed to `Surface(viewModel:catalogs:)` | This is where ForgeUI comes in | Good fit: an ixOS catalog's `Card` can be a `ForgeCard`, a row a `ForgeRow` |

## Compared with ixOS

### What ixOS has today

- **ForgeUI and ForgeAppKit**: fixed, reusable components (ForgeCard, ForgeRow, ForgeBadge, ForgeNotice, ForgeRename, ForgeStateView, ...). They stay exactly as they are. A2UI would *use* them as the rendering of catalog components; it never replaces them.
- **AI**: `forge-services` `ai` (`ChatRequest`, the `LanguageModel` foreign trait, Apple's on-device model as the default backend), AI over Relay (`relay ask`, `serve-ai`), and the Bonsai Harness in Device Lab (llama.cpp with **GBNF grammars generated from a tool list**, so a 4B model can only answer with a valid call).
- **No agent-composed UI anywhere.** Every assistant answer in ixOS is text (Relay's Assistant, Pocket Library's chapter summaries, Urdu Naturally's planned conversation).

### How it would fit the architecture

Reuben's layering holds, with one adjustment to who holds which state:

| Layer | ixOS owner | Holds |
|---|---|---|
| Agent / model | Rust: a session per surface in the app's crate | The conversation, the model calls, which catalog it may use |
| Wire messages | Rust builds or checks them | A2UI v1.0 JSON |
| Function registry | **Rust** | Each registered app function, its argument schema, and whether it may only *propose* |
| Surface state | Swift, inside A2UI's `MessageProcessor` | The component tree and the surface's data model: the draft the person is editing |
| Rendering | Swift: A2UI SwiftUI adapter + an **ixOS catalog** of ForgeUI-backed components | Views only |
| The actual change | Existing app logic in Rust, after the person confirms | The app's real data |

The surface's data model living in Swift is a deviation from "Rust owns state", in the same class as an unsaved form's text fields: it is a draft, never the app's data. It needs to stay that way. Every event and function call goes back through the FFI to Rust, which validates it against its own registry before anything else happens.

### The security rules Reuben set, mapped onto the protocol

| Rule | How it holds |
|---|---|
| No arbitrary code execution | A2UI carries no code by design. The ixOS catalog adds **no** expression or scripting functions beyond the basic validation and formatting ones |
| Generated UI calls only explicitly registered app functions | The ixOS catalog lists them; the Swift renderer rejects anything else; Rust rejects it again (the agent is untrusted, so is the renderer's input) |
| No unrestricted app APIs | Functions default to `rendererOnly`. **No ixOS function is `agentOnly` or `rendererOrAgent` in the first milestones**: the agent can draw, not act |
| No direct database writes from generated UI | Registered functions only *propose* (for example "plan these books for these dates"), returning a change set. Applying it is a separate step |
| The person confirms | The **Confirm** control is app-owned and sits **outside** the agent's surface, so an agent can't draw a fake one, pre-press one or hide what it does. It shows the change set from Rust, not from the surface |

Two risks the A2UI docs don't make obvious for an on-device personal app:

1. **Remote images are a data leak.** `Image`, `Video` and `AudioPlayer` take URLs. An agent that has read your library could put it in an image URL, and showing the surface would send it. The ixOS catalog leaves these out, or allows only app-supplied local references.
2. **`openUrl`** stays without a handler (A2UI's Swift default), so it opens nothing.

And one ixOS rule: **never log user content** (AGENTS.md rule 5). Surfaces are full of it, so A2UI payloads are never logged; only counts, ids and error codes.

### Which agent can drive it

| Agent | Can it produce A2UI? | Notes |
|---|---|---|
| A **deterministic Rust builder** (no model) | Yes, and the A2UI docs endorse it | The model fills a small app-specific structure (a reading plan: books, dates, priorities); Rust turns it into A2UI. Safest and most reliable first step |
| **Apple's on-device model** (`forge-services` `ai`) | Through guided generation (`@Generable`), into that same small structure | Free-form A2UI from a ~3B model is unproven |
| **Bonsai 4B/9B** (llama.cpp) | Through a GBNF grammar, as the harness does for tool calls | The grammar could come from the ixOS catalog, like `calls.rs` does from tools. Express (`Express.g4`) is an ANTLR grammar; there is no Rust or Swift compiler for it, so using Express means writing one |
| The **Python agent SDK** in Python Lab | **No.** It needs pydantic (compiled pydantic-core) and jsonschema; there are no iOS wheels, the same wall as numpy in the [visual labs spec](../../../../../examples/device-lab/docs/visual-labs-spec.md) | |
| A laptop model over **Relay** | Yes (any SDK, full models) | Possible later for big jobs; not the first step |

So the first agent should **not** compose free-form UI. It should fill a typed plan that Rust renders as A2UI, and free composition comes later, once the catalog and the function boundary are proven.

### Android

The Compose renderer is official but **v0.9.1 only**, and v1.0 is the version with the function calls and theme removal that ixOS's security model leans on. Android in ixOS is also a capped side track (Pocket Library only, one milestone at a time, the Android phone under repair, emulator CI only). Rendering "the same interface on iOS and Android" (A2UI-2) would add new Android scope beyond Pocket Library, so it is Reuben's call, and it is best done after Compose reaches 1.0 and Android's own A8.

### Product fit

Reuben's four app examples all have the same shape, which is what A2UI is good at: an agent drafts something structured, the person adjusts it, the app applies it. The reading-plan flow fits Pocket Library's planned reading goals ([app-plan.md](../../../../app-plan.md)); the research brief fits Commonplace's reading system. Checklist and FieldKit (generated checklists, review before apply) and Soundscape (dynamic playlists) follow the same pattern. A2UI does not help with screens that are always the same; those stay ForgeUI.

## Scorecard

| Dimension | Rating | Reason |
|---|---|---|
| Capability | Strong | Agent-composed native UI with a declared function boundary; ixOS has nothing like it |
| Product value | Adequate | Real for Pocket Library and Commonplace, but only once an on-device model can produce something worth showing; unproven on a 4B model |
| Architecture | Adequate | Fits beside ForgeUI (catalog components render as ForgeUI); the surface data model in Swift is a contained deviation |
| Platform | Adequate | iOS 17+, the same as ixOS. Not run |
| Development | Unknown | Not built with xtool. Build-tool plugin and swift-syntax resolution are the unknowns. Core isn't testable on Linux (Combine) |
| Maintenance | Strong | Google-backed, official Swift and Compose renderers, conformance suites, frequent releases |
| Licence | Strong | Apache-2.0, dependencies MIT and Apache-2.0; compatible with MIT ForgeKit and apps |
| Security | Adequate | Good protocol model (catalog, `allowedCallers`, user activation). Remote media and a Confirm the agent can draw are holes ixOS must close itself |
| Integration cost | Weak | About 26k lines plus the first third-party Swift dependency, for one experiment |
| Reusability | Strong | Every app with an assistant could use the same catalog and boundary |

## Verdict

**Study, with a build spike before any Adopt.** The protocol is a good wire format and security model for agent-composed UI, and the official SwiftUI renderer is the right thing to try before writing anything of our own. But it hasn't built under xtool, it would be ixOS's first third-party Swift package, and no ixOS agent can yet produce the UI it renders. Nothing enters ixOS from this assessment. A2UI-1 (below) answers the build question in Device Lab only.

Reopen when: the A2UI-1 spike builds (or fails) under xtool; Compose reaches A2UI 1.0; or a second app wants agent-composed UI after the first.

## Next step: proposed A2UI-1 scope (waits for Reuben's approval)

**A2UI Lab: one fixed reading-plan surface, rendered natively in Device Lab, no model.** One PR, Device Lab only, nothing in ForgeKit or any app.

1. **Build spike.** Add the official package (`0.11.0`, `A2UISwiftCore` + `A2UISwiftUI` only, not the basic SwiftUI catalog) to Device Lab and build it with `scripts/check-ios-xtool.sh` on the laptop. Record whether the plugin and swift-syntax resolution work, and the app-size change. If the plugin fails: stop and report, with vendoring the generated files as the option.
2. **An ixOS catalog** (`catalog.json` in Device Lab, about eight components): Text (ForgeUI text roles), Card (ForgeCard), Row (ForgeRow), Badge (ForgeBadge), Column, Button, DateTimeInput, ChoicePicker or Slider for priority. No Image, Video, AudioPlayer or Modal; `openUrl` without a handler. Every control gets an `a2ui.<surface>.<id>` identifier and a VoiceOver label.
3. **A fixed fixture**: the reading-plan surface from Reuben's scenario, as v1.0 JSON written by hand, plus a second message that updates it (a later date, a reordered book) to show incremental updates.
4. **Rust side** in Device Lab's crate: loads the fixtures, receives the surface's events over the FFI, and checks every event name and its context against a registry. **One** registered function, `readingPlan.propose`, which returns a change set and writes nothing. Rust tests on Linux cover the registry rejecting unknown names, malformed context and anything not `rendererOnly`.
5. **App-owned Confirm** below the surface, showing the change set from Rust. In A2UI-1 it only shows "Would apply: ..."; it doesn't touch Pocket Library.
6. **Phone check (needs the iPhone, ~10 min, rides the next combined Device Lab build):** the surface looks like the rest of Device Lab in light and dark and at the largest text size; VoiceOver reads the controls; the update message changes only what it should; an unknown event is refused with a visible notice; Confirm shows the change set.

What A2UI-1 deliberately leaves out: any model (A2UI-4), Android (A2UI-2), agent-callable functions, any real app data (A2UI-5), and any ForgeKit module (only after a second app uses it, per the apps-first rule).

### Open decisions for Reuben

1. **The first third-party Swift dependency.** Allowing `a2ui` (and `swift-json-schema`, `swift-collections`) into Device Lab's `Package.swift` for the spike. The alternative is writing an ixOS renderer for a narrow slice of the v1.0 wire format on ForgeUI, which is reinvention and only worth it if the spike fails.
2. **A2UI-2 (Android).** Recommend **deferring** it until Compose supports A2UI 1.0 and Android's A8 is done, since it would add Android scope beyond Pocket Library.
3. **A2UI-4's first agent.** Recommend a typed plan filled by Apple's on-device model or Bonsai and rendered by Rust, before free-form UI from a model.
