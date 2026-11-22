# A2UI: agent-composed native interfaces beside ForgeUI?

Status: 2026-11-22. Part of the [External Projects Atlas](../../README.md). Milestone A2UI-0 of Reuben's A2UI plan (2026-11-22). Reuben approved A2UI-1 the same day with the decisions and acceptance conditions [below](#decisions-reuben-2026-11-22); A2UI-1 is built in Device Lab ([A2UI-1 as built](#a2ui-1-as-built)).

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
| No unrestricted app APIs | Functions default to `rendererOnly`, which stops the agent calling them directly but not the surface: a `rendererOnly` function bound into a component can run without a tap unless it also has `requiresUserActivation`. So **no ixOS function is `agentOnly` or `rendererOrAgent` in the first milestones**, any later propose function sets `requiresUserActivation`, and the A2UI-1 catalog declares **no functions at all** (Rust's gate also refuses `@call` and `functionCall`): the agent can draw, not act |
| No direct database writes from generated UI | Registered functions only *propose* (for example "plan these books for these dates"), returning a change set. Applying it is a separate step |
| The person confirms | The **Confirm** control is app-owned and sits **outside** the agent's surface, in the review sheet's toolbar. An agent can draw a button *labelled* Confirm inside its surface, but that button can only send the one registered review event, which opens the review; it can't apply anything, pre-press the real Confirm or hide what it does. The real one is told apart by place (the sheet the app opens, never the surface) and content (the change set from Rust, not from the surface) |

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

## Next step: proposed A2UI-1 scope (approved 2026-11-22)

**A2UI Lab: one fixed reading-plan surface, rendered natively in Device Lab, no model.** One PR, Device Lab only, nothing in ForgeKit or any app.

1. **Build spike.** Add the official package (`0.11.0`, `A2UISwiftCore` + `A2UISwiftUI` only, not the basic SwiftUI catalog; as built it also takes `BasicCatalog` and `BasicCatalogSwiftUI`, see below) to Device Lab and build it with `scripts/check-ios-xtool.sh` on the laptop. Record whether the plugin and swift-syntax resolution work, and the app-size change. If the plugin fails: stop and report, with vendoring the generated files as the option.
2. **An ixOS catalog** (`catalog.json` in Device Lab, about eight components): Text (ForgeUI text roles), Card (ForgeCard), Row (ForgeRow), Badge (ForgeBadge), Column, Button, DateTimeInput, ChoicePicker or Slider for priority. No Image, Video, AudioPlayer or Modal; `openUrl` without a handler. Every control gets an `a2ui.<surface>.<id>` identifier and a VoiceOver label.
3. **A fixed fixture**: the reading-plan surface from Reuben's scenario, as v1.0 JSON written by hand, plus a second message that updates it (a later date, a reordered book) to show incremental updates.
4. **Rust side** in Device Lab's crate: loads the fixtures, receives the surface's events over the FFI, and checks every event name and its context against a registry. **One** registered function, `readingPlan.propose`, which returns a change set and writes nothing. Rust tests on Linux cover the registry rejecting unknown names, malformed context and anything not `rendererOnly`.
5. **App-owned Confirm** below the surface, showing the change set from Rust. In A2UI-1 it only shows "Would apply: ..."; it doesn't touch Pocket Library.
6. **Phone check (needs the iPhone, ~10 min, rides the next combined Device Lab build):** the surface looks like the rest of Device Lab in light and dark and at the largest text size; VoiceOver reads the controls; the update message changes only what it should; an unknown event is refused with a visible notice; Confirm shows the change set.

What A2UI-1 deliberately leaves out: any model (A2UI-4), Android (A2UI-2), agent-callable functions, any real app data (A2UI-5), and any ForgeKit module (only after a second app uses it, per the apps-first rule).

### Open decisions for Reuben (all decided, below)

1. **The first third-party Swift dependency.** Allowing `a2ui` (and `swift-json-schema`, `swift-collections`) into Device Lab's `Package.swift` for the spike. The alternative is writing an ixOS renderer for a narrow slice of the v1.0 wire format on ForgeUI, which is reinvention and only worth it if the spike fails.
2. **A2UI-2 (Android).** Recommend **deferring** it until Compose supports A2UI 1.0 and Android's A8 is done, since it would add Android scope beyond Pocket Library.
3. **A2UI-4's first agent.** Recommend a typed plan filled by Apple's on-device model or Bonsai and rendered by Rust, before free-form UI from a model.

## Decisions (Reuben, 2026-11-22)

"Promising enough to prototype, but not yet proven enough to adopt." A2UI-1 goes ahead as proposed, deliberately small: eight components, one fixture, one update, one propose-only function, one app-owned confirmation flow.

1. **First third-party Swift dependency: approved for Device Lab only.** Use the official Swift renderer, not a home-grown one. Keep it out of ForgeKit and production apps. Record build time, package resolution, generated-code (build plugin) behaviour and app-size impact. If the xtool build fails, **stop and report**: don't vendor the generated files or rewrite the integration. This is permission to test the dependency, not to adopt it.
2. **Android deferred.** Android A2UI waits until Google's Compose renderer supports A2UI 1.0 **and** ixOS Android A8 is done. Prove iOS first.
3. **Typed structure before free-form UI.** The progression: a model produces a typed reading-plan proposal; Rust validates its structure and constraints; Rust generates the A2UI messages; Swift renders them with the registered ForgeUI-backed components; the person edits the draft; Rust validates the proposed changes; the app-owned Confirm presents the actual change set; existing app logic applies it only after confirmation. If a fixed, typed structure gives a genuinely better interaction, that is the reason to explore more flexible generation later.

### A2UI-1 acceptance conditions

| Area | Requirement |
|---|---|
| Invalid input | Unknown component types, event names and malformed payloads are rejected safely |
| Privacy | No user content or complete A2UI payloads in logs |
| Accessibility | VoiceOver and Dynamic Type must work, not merely pass a visual inspection |
| Confirmation | Confirm displays a Rust-derived change set, never a summary supplied by the A2UI surface |
| Updates | An incremental update can't silently change the meaning of an existing action, or alter the proposed change set without the confirmation view reflecting it |

## A2UI and ForgeUI (Reuben, 2026-11-22)

They overlap but solve different problems. **ForgeUI defines what an ixOS interface looks and feels like; A2UI defines how an agent describes an interface for the app to render.** A2UI is a possible *consumer* of ForgeUI, never a replacement and never a second design system. The rule to protect: **A2UI decides which interface to compose; ForgeUI decides how it looks and behaves.**

| Layer | Owns | Where |
|---|---|---|
| Agent / Rust | Which interface is useful; the typed proposal; the gate | App crate (`a2ui_lab/` in Device Lab) |
| A2UI runtime | Message processing, streaming, data binding, validation | The integration layer (A2UI's `A2UISwiftCore` / `A2UISwiftUI`, driven by the app), **not** ForgeUI |
| ForgeUI A2UI adapter | Maps each permitted A2UI component to a ForgeUI component, or to native SwiftUI on ForgeUI's tokens where ForgeUI has no component | Device Lab for now (`A2uiLabSurface.swift`); its own module beside ForgeUI only after a second app uses it |
| ForgeUI | Components, tokens, type, accessibility behaviour | `forgekit/Sources/ForgeUI`, unchanged, with no A2UI dependency |

Three rules follow:

1. **ForgeUI stays independent.** It never imports A2UI and needs nothing from it. `scripts/check-boundaries.sh` already keeps A2UI out of every package but Device Lab.
2. **A2UI components resolve to ForgeUI.** Every component in an ixOS catalog is drawn by the adapter, not by A2UI's own views. Google's `BasicCatalog` is used for its component *definitions* (the v1.0 schema the gate and renderer validate against); `BasicCatalogSwiftUI`'s *look* is acceptable in the protocol spike only and must not ship in any surface a person relies on.
3. **The A2UI runtime stays in the integration layer.** No message processing, data binding or validation moves into ForgeUI's core.

### Where A2UI-1 stands against this

| Component | A2UI-1 (#377) | A2UI-1b |
|---|---|---|
| Text | ForgeUI type roles (`.forge(.body)`, `.metadata`) | Same |
| Card | `ForgeCard`, through A2UI's card style hook | `ForgeCard`, drawn by the adapter |
| Button | A2UI's button with a ForgeUI style | A native button on ForgeUI's accent, radius and 44 pt height (native button styles on ForgeUI tokens are allowed) |
| Column, Row | A2UI basic catalog stacks | Stacks on ForgeUI spacing; a row that doesn't fit across stacks instead of clipping |
| Divider | A2UI basic catalog | A hairline in ForgeUI's separator colour |
| ChoicePicker (priority) | A2UI basic catalog chips | ForgeUI chips on the accent tone, ticked when selected, with the selected trait for VoiceOver |
| DateTimeInput | A2UI basic catalog date field | Native `DatePicker` with a ForgeUI label, tinted with the accent; dates only |

A2UI-1 answered the protocol questions (build, gate, confirmation, logs) with three of the eight drawn through ForgeUI. **A2UI-1b** (`A2uiForgeCatalog.swift`) draws all eight through the adapter, so Device Lab no longer depends on `BasicCatalogSwiftUI` at all: `BasicCatalog` supplies only the v1.0 component definitions the renderer validates against. Rust's gate now also refuses any property the adapter doesn't draw (`unsupported_property`: a time picker, a filter field, a checkbox list, an unknown property), so an agent can't ask for a look the adapter lacks. ForgeUI itself doesn't change. The review sheet labels a book with no saved plan "New plan" rather than counting it as a change (phone check finding, step 2).

**A2UI-1b phone check (Reuben, 2026-11-22, Device Lab 1.29922445, #385 at `862b178`): all four steps pass.** (1) Cards, chips, date fields and divider look like the rest of Device Lab in light and dark, with the selected chip filled and ticked; (2) VoiceOver reads each chip's name and selected state; (3) at a large text size the Start and Finish dates stack instead of clipping; (4) a first review shows "New plan" rows, counted apart from changes.

**Next (Reuben, 2026-11-22):** no more A2UI components for now; the eight prove the architecture, and the adapter stays in Device Lab. The proposed next milestone is an **app capability audit**: the typed actions and data each app could safely expose to A2UI, so an agent composes interfaces around real app capabilities. It starts only when Reuben says so.

**Longer term**, an adapter shared through ForgeKit (after a second app, per the apps-first rule) would give every ixOS app the same kind of agent-composed surface, a reading plan in Commonplace, a task flow in Checklist, a queue in Soundscape, while each still looks and behaves like an ixOS app.

## A2UI-1 as built

Device Lab, Tools → **A2UI Lab**. Rust in `examples/device-lab/rust/src/a2ui_lab/`, Swift in `examples/device-lab/Sources/AppHost/A2uiLab*.swift`.

| Step | Where | What it does |
|---|---|---|
| Typed proposal | `plan.rs` | `Proposal` holds book ids, priorities and dates only (no titles, no interface). Checked: known books, no repeats, 1 to 12 books, real dates, finish not before start, at most a year. A2UI-1's proposal and the agent's revision are fixed JSON, standing in for a model |
| Messages | `surface.rs` | The same fixed tree every time: a book card template (title, author, priority chips, start and finish dates) and one Review button. The proposal only changes data |
| Gate | `gate.rs` | Every agent-to-renderer payload, before Swift sees it: A2UI 1.0 only, the lab's surface and catalog, the eight components, no `@call` / `functionCall` / `callRendererFunction` / `deleteSurface` / `@catalogId`, only the properties the ForgeUI adapter draws (dates only, chips only; since A2UI-1b), the one registered event from the one registered button with exactly its context and no `userMessage`, no change to an existing component's action, after creation no redrawing of any component but the intro text (so an update can't relabel the Review button), data only at `/books` holding known books with their own titles and authors. All or nothing: a refused payload changes nothing |
| Rendering | `A2uiLabSurface.swift`, `A2uiForgeCatalog.swift` | `MessageProcessor` with `ValidationConfig.strict` and the lab's catalog of eight v1.0 components (Text, Column, Row, Card, Divider, Button, DateTimeInput, ChoicePicker), every one drawn by the ForgeUI adapter (since A2UI-1b); no functions at all |
| Review | `A2uiLab::review` | The Review button's event comes back through the renderer's outbound listener. Rust checks the event name and source, the edited plan (same books as the proposal, one priority each, valid dates), and derives the change set from **its own books**: titles and authors from the library, plans from Rust's check, "Replaces" from what was saved |
| Confirm | `A2uiReviewSheet` | App-owned sheet, outside the surface, showing only Rust's change set. Every review gets its own revision, and `confirm(revision)` applies only that review's change set, and only if nothing changed since: an agent update or an edit on the surface drops the reviewed change set, and Confirm then refuses and asks for a new review. It writes a stand-in library only |
| Logs | `mod.rs` | Codes and counts only (`refused: action_changed`, `reviewed: 4 changes`); never a payload, a title or a date |

Tests (Linux, `cargo test --manifest-path examples/device-lab/rust/Cargo.toml --lib a2ui_lab`, 16 tests): every hostile payload refused with the right code and nothing changed; the lab's own surface passes its gate with eight component types and exactly one action; titles in the change set come from the library even when the surface's data says otherwise; an update or an edit after review makes Confirm refuse; Confirm needs a review and applies once.

**Wider than the proposed scope.** The proposal said `A2UISwiftCore` + `A2UISwiftUI` only. The lab also depends on `BasicCatalog` and `BasicCatalogSwiftUI` (the same package, the same official renderer), because the eight components are *derived* from A2UI's own v1.0 basic catalog rather than reimplemented, which is what "use the official renderer, not a home-grown replacement" asked for. In A2UI-1, Text, Card and Button were drawn through ForgeUI and the other five used the basic catalog's views; A2UI-1b draws all eight through ForgeUI and drops `BasicCatalogSwiftUI` (see [A2UI and ForgeUI](#a2ui-and-forgeui-reuben-2026-11-22)). The build numbers below measure A2UI-1's wider graph.

The rule that only Device Lab may use A2UI is checked: `scripts/check-boundaries.sh` fails on any third-party Swift package outside its allowlist, which holds A2UI in `examples/device-lab/Package.swift` only.

### Build spike record

| Item | Result |
|---|---|
Run 2026-11-22 on the laptop: xtool 1.24.0, Swift 6.2.3, `scripts/check-ios-xtool.sh --ref claude/project-thread-hzo4mj device-lab`, unsigned, no phone.

| Item | Result |
|---|---|
| Swift compile of the lab | **Passes** under xtool, first time, at 7923015 and again at ddc9009 |
| Package resolution | About 3 minutes, all from the laptop's SwiftPM cache: a2ui 0.11.0+swift 131.5 s (the whole monorepo, 240,700 objects), swift-json-schema 0.13.2 34 s, swift-syntax 604.0.0 10.5 s, swift-collections 1.7.6 2.3 s |
| swift-syntax | Fetched and checked out, but **not compiled**: no SwiftSyntax, SwiftParser or macro module in the log |
| Modules compiled | A2UICore (86 files), JSONSchema (34), A2UISwiftUI (9), A2UIJSON (3), A2UISchemaGenerator (1), plus the basic catalog |
| Build-tool plugin (`A2UISchemaGeneratorPlugin`) under xtool | **Works.** The generator links, the plugin compiles and runs, and writes `GeneratedA2UIJSONSchemas.swift` (83 KB) and `GeneratedBasicCatalogComponents.swift` (103 KB) into the build directory. Nothing vendored |
| Build time | Clean: 797 s in all (resolution about 3 min, Rust 4 min 7 s in a fresh worktree, Swift 267 s). Incremental after a branch change: 112 s (Swift 61 s), nothing fetched |
| App size (debug) | 193 MB against 184 MB for the last combined build (fd095d77): **+9.1 MiB, about 5%**, an upper bound since the branch differs from that build in other ways too |

### Phone check (needs the iPhone, about 10 minutes)

Proves the acceptance conditions on the device; only Reuben's report counts.

1. Tools → A2UI Lab → **Show the Proposal**: four book cards in ForgeUI's look, light and dark.
2. Change one book's priority and one date, then **Review changes**: the sheet lists only Rust's rows (titles from the library) and Confirm saves them; **Saved plans** shows them.
3. Review again without changes: "Nothing would change", Confirm disabled.
4. **Send the Agent's Update**: the intro text and the dates change, and nothing else does. Review: the rows show the agent's new dates. (Confirming a review made before an update or an edit is refused; the sheet covers the controls, so that path is proved by the Rust tests.)
5. **Send a Bad Payload**, each of the eight: each is refused with a reason and the cards don't change.
6. VoiceOver: each card reads its book title; the priority chips read their state; the date fields and Review button are reachable and named. Largest Dynamic Type size: nothing clipped, the screen scrolls.
7. Tools → Logs, filter `a2ui_lab`: only codes and counts, no titles, dates or payloads.

**Result (Reuben, 2026-11-22, Device Lab 1.29922371 = main + #377 at `3df9ab9`): all seven steps pass.**

| Step | Result |
|---|---|
| 1. Proposal, light and dark | Pass |
| 2. Edit, review, confirm, saved plans | Pass. The first review listed all four books: correct, since nothing was saved yet and Rust compares with what's saved, but the sheet should say so; A2UI-1b labels first-time rows "New plan". A later single priority change showed one row |
| 3. Review with no changes | Pass: "Nothing would change", Confirm disabled |
| 4. Agent's update | Pass |
| 5. Eight bad payloads | Pass: all refused, cards unchanged |
| 6. VoiceOver and large text | Pass |
| 7. Logs | Pass: codes and counts only |

All five A2UI-1 acceptance conditions hold on the device. This passes the test of the dependency; it does not adopt it (decision 1).
