# Candidate catalogue

Status: v0.1, 2026-11-17. Part of the [External Projects Atlas](README.md). Seeded from Reuben's list of the same day.

A **discovery shortlist**, not a claim that any repository has passed a source, security, licence or maintenance audit. Projects marked *unverified* have not been inspected yet: their descriptions, licences and platform claims are as proposed, and they become verdicts only through an [assessment](evaluation-framework.md#assessment-template). Projects marked **prior** were already investigated: their row links the [register](prior-research-register.md), which is where their decision lives.

## Classifications

Provisional recommendations, not adoption decisions.

| Mark | Meaning |
|---|---|
| **Candidate** | Worth a contained integration or prototype |
| **Technique** | Reuse a technique, design, algorithm or workflow, not the code |
| **Reference** | Valuable to study, but not an obvious dependency |
| **Poor fit** | Superseded, redundant or unsuitable, with the reason |
| **Prior** | Already examined; don't restart, see the [register](prior-research-register.md) |

Priority (P0 to P2) is from [Proposed research priorities](#proposed-research-priorities) below; a blank means not scheduled.

## 1. Networking and device communication

Relay is the natural beneficiary, but networking could also serve cross-device Chat, remote control and shared experiences. **Question:** can Relay become a general device-to-device transport rather than a file-transfer app?

| Project | Potential contribution | Classification | Priority |
|---|---|---|---|
| Iroh | P2P connections, NAT traversal, relay fallback, content transfer | **Prior:** adopted in Relay (#197, #198, #267) | |
| LocalSend | Discovery, pairing, device selection, transfer UX | **Prior:** UX reference (#288); rejected as protocol | |
| libp2p | Modular peer discovery and transport protocols | **Prior:** not chosen for Relay (more moving parts, weaker iOS story). Revisit only as a comparison | P2 |
| Lightning P2P | iroh-based transfer: verified chunks, retry and resume, browser receiving | **Technique**, *unverified*. Its iOS support is advertised but not available, so an architectural reference, compared against Relay's outbox (#299) | P0 |

## 2. Local-first data and synchronisation

Potentially the most consequential area. **Opportunity:** one sync capability so people move between their own devices without each app inventing backup, export and replication. **Constraint:** no second persistence architecture. The Rust data layer (`forge-storage`) is the baseline; a sync technology has to prove what it adds.

| Project | Potential contribution | Classification | Priority |
|---|---|---|---|
| Automerge | CRDTs, offline changes, conflict-free merges (Rust core) | **Candidate** for an architecture investigation, *unverified* | P1 |
| Yjs | Collaborative shared data structures (JavaScript; Rust port y-crdt) | **Reference**, *unverified* | P2 |
| ElectricSQL | Local-first Postgres sync | **Reference**, *unverified*. Needs a Postgres server, which ixOS apps don't have | P2 |
| Syncthing | Device discovery, folder replication, conflict handling | **Prior:** a different product; watch for a later folder-sync mode in Relay | P2 |

## 3. Security and privacy

The opportunity is making privacy understandable: what is stored, who can access it, which device is trusted, what happens after revocation, what can be recovered.

| Project | Potential contribution | Classification | Priority |
|---|---|---|---|
| Signal-iOS | Secure messaging, identity, key lifecycle, safety UX | **Prior:** experience reference for Chat (#285), audit not yet done. AGPL: behaviour only | P2 |
| libsodium | Cryptographic primitives | **Reference**, *unverified*. ixOS already uses ring, rustls and snow in Rust; a C library would duplicate them | |
| RustCrypto | Rust cryptographic implementations | **Prior:** `spake2` adopted in Relay | |
| age | Simple file encryption and key handling | **Reference**, *unverified*. RelayPonyCore uses it (#297); relevant if Relay ever stores files encrypted at rest | |

## 4. On-device AI and intelligent features

**Opportunity:** a private intelligence service offering transcription, semantic search, classification, summarisation and natural-language actions to individual apps, which keep working without it. Existing work: [local-ai.md](../../local-ai.md) and [forge-ai-plan.md](../../forge-ai-plan.md).

| Project | Potential contribution | Classification | Priority |
|---|---|---|---|
| MLX Swift | Apple Silicon inference, LLM and VLM examples | **Prior:** poor fit for Linux builds (its Metal shaders need Xcode). A spike has to answer that first | P1 |
| FluidAudio | Local speech recognition, diarisation, speech processing on Core ML | **Assessed 2026-11-17: Watch.** Wrap Apple's on-device speech recognition first; FluidAudio when an app needs diarization or VAD ([assessment](assessments/ai/fluidaudio.md)) | P0 |
| WhisperKit | On-device transcription | **Reference**, *unverified*. Same overlap with Apple's speech recognition as FluidAudio's transcription ([FluidAudio assessment](assessments/ai/fluidaudio.md)) | |
| llama.cpp | Quantised local inference | **Prior:** fallback adapter after CoreML-LLM; laptop upstream for AI over Relay | |

MLX's Apple Silicon support does not make it a drop-in for every iPhone, or for the Linux build machine.

## 5. Accessibility and interaction quality

Probably the best immediate return: improvements reach every app. **Opportunity:** an accessibility gate next to the existing build and test checks, catching missing labels and poor control semantics before the phone.

| Project | Potential contribution | Classification | Priority |
|---|---|---|---|
| CVS Health SwiftUI Accessibility Techniques (a11y-check) | A rule-based accessibility analyser (47 rules), good and bad examples, automatic fixes, CI | **Assessed 2026-11-17: Adopt as a CI tool**, report-only first; builds and tests on Linux ([assessment](assessments/accessibility/a11y-check.md)) | P0 |
| Swift Snapshot Testing | Visual and structural regression tests | **Prior:** simulator only; ideas borrowed into phone baselines | |
| AccessibilitySnapshot | Accessibility hierarchy snapshots | **Prior:** simulator only; tree-diff idea pending. Reassess next to a11y-check | |
| ViewInspector | Inspect and test SwiftUI views | **Reference**, *unverified*. Needs SwiftUI at test time, so a Mac or simulator, not Linux CI | P2 |

## 6. Images, documents and visual media

Relevant to Pocket Library, Commonplace, Sketchbook and Relay.

| Project | Potential contribution | Classification | Priority |
|---|---|---|---|
| OpenScanner | A complete native SwiftUI scanning app (Core Data, CloudKit) | **Reference**, *unverified* | |
| Offscan | Offline scanning, OCR, non-destructive editing, PDF export, on Apple frameworks | **Technique**, *unverified*: study the pipeline and architecture, not a new scanner app | P0 |
| Brightroom | Composable image editing on Core Image and Metal | **Candidate**, *unverified* (Sketchbook, Relay previews) | P1 |
| Kingfisher | Image loading, caching and processing | **Poor fit:** the Nuke decision covers it; ForgeImages is extended instead (#298). Not a reason to reopen that decision | |

## 7. Audio, speech and music

Soundscape is now a music player for generated songs; analysis matters more than synthesis.

| Project | Potential contribution | Classification | Priority |
|---|---|---|---|
| AudioKit | Synthesis, effects, audio processing | **Reference**, *unverified*. Soundscape's studio screens are being removed | |
| FluidAudio | Speech and audio intelligence | See section 4 | P0 |
| WhisperKit | Transcription and audio indexing | See section 4 | |
| Essentia | Music information retrieval and audio analysis | **Reference**, *unverified*: licence and C++ integration to check before anything else | P2 |

## 8. Reading, knowledge and personal libraries

**Opportunity:** Commonplace becomes a personal knowledge environment without turning into a generic notes app.

| Project | Potential contribution | Classification | Priority |
|---|---|---|---|
| Readest | Reading experience, annotations, dictionaries, parallel reading, library management | **Reference**, *unverified*. Not SwiftUI; study the product design | P1 |
| NetNewsWire | RSS navigation and repeated reading workflows | **Prior:** product benchmark (#293) | |
| Ente | Private media library, encrypted sync | **Reference**, *unverified* | |
| Anytype | Structured personal knowledge, local-first organisation | **Reference**, *unverified* | |

## 9. Maps, geography and fieldwork

**Opportunity:** FieldKit as a field instrument: offline maps, observations, routes, measurements, photos, timestamps, personal geographic history. The question is what is useful in the field, not how many layers to show.

| Project | Potential contribution | Classification | Priority |
|---|---|---|---|
| MapLibre Native | Custom and offline-capable maps | **Candidate**, *unverified*: xtool build of its iOS framework is the first question | P1 |
| Valhalla | Routing and navigation engine | **Reference**, *unverified* | |
| GPXSee | GPS track viewing and analysis | **Reference**, *unverified* (desktop) | |
| OpenStreetMap | Open geographic data | **Reference**, *unverified*: a data source, with its own licence terms | |

## 10. Automation and Apple system integration

**Opportunity:** a shared action vocabulary: save to Commonplace, send through Relay, start Soundscape, record a FieldKit observation, without opening the app. More interesting than a widget per app.

| Project | Potential contribution | Classification | Priority |
|---|---|---|---|
| Apple App Intents sample | Spotlight, Shortcuts, Siri, system actions | **Prior:** the Linux build gap is known ([app-intents-on-linux.md](../app-intents-on-linux.md)); its two checks come before any design | P1 |
| WidgetExamples | Widgets, interactive controls, Live Activities | **Reference**, *unverified* | |
| Ice Cubes | Real-world App Intents, extensions, system integration | **Prior:** product benchmark (#291) | |
| Shortcuts | User-defined automation | **Reference:** the system app App Intents serve | |

## 11. Developer tooling, testing and observability

The goal is testing real user journeys, not only whether views render. Also study the testing strategies of Ice Cubes and OpenScanner, not only their libraries.

| Project | Potential contribution | Classification | Priority |
|---|---|---|---|
| SwiftLint | Swift code quality and custom rules | **Candidate**, *unverified*: it has a Linux build; would sit beside `forge check swift` | |
| Maestro | End-to-end mobile UI automation | **Prior:** no physical iPhone support (2026-11-15) | P2 |
| Pulse | Network inspection and logging | **Prior:** idea only | |
| Swift Testing | Modern Swift test framework | **Candidate**, *unverified*: ships with the Swift 6 toolchain on Linux; ixOS's Swift tests use XCTest today (30 files) | |

## 12. Architecture and complete product references

Judged as product references, not as reusable code.

| Project | Potential contribution | Classification | Priority |
|---|---|---|---|
| Ice Cubes | Native SwiftUI architecture and system integration | **Prior** (#291) | |
| Signal-iOS | Messaging, privacy, trust and recovery | **Prior** (#285) | P2 |
| Bitwarden | Cross-platform consistency and vault UX | **Reference**, *unverified* | P2 |
| OpenScanner | A small, complete native app | **Reference**, *unverified* | |

## Counts

48 rows, 43 distinct projects (FluidAudio, WhisperKit, Ice Cubes, Signal-iOS and OpenScanner appear twice). 15 are prior work; 28 are new and unverified (Kingfisher's poor fit follows from the Nuke decision, not from an inspection). The 2026-11-17 proposal said 50 and 45; these are the counts of the rows it listed, all kept here.

## What is genuinely interesting in this batch

| Project | Why | Next step |
|---|---|---|
| a11y-check | A direct improvement to every app's quality gate | P0: can its rules run on Linux as a mandatory check? |
| FluidAudio | Speech for several apps on Apple's hardware, without each app building it | P0: xtool build and an iPhone run |
| Lightning P2P | Unusually relevant to Relay's outbox, resume and transfer design | P0: compare with #299 |
| Offscan | A sophisticated app built on Apple's frameworks, not a dependency stack | P0: study the pipeline |
| Automerge | Could change how apps share data, work offline and merge | P1: test against forge-storage |
| MLX Swift | An optional on-device intelligence layer | P1, but start from the Linux build finding in local-ai.md |

## Proposed research priorities

Not every project gets the same depth; that would produce documentation without improving ixOS.

**P0, immediate (one assessment PR each, one at a time):** a11y-check (quality gates), FluidAudio (shared local speech), Lightning P2P (against Relay's outbox), Offscan (document pipeline and architecture).

**P1, capability investigations:** Automerge against Rust persistence; MLX Swift on a supported iPhone, if the Linux build question has an answer; Readest for reading and annotation; Brightroom for image editing; MapLibre for FieldKit's maps; App Intents for cross-app actions (the existing spike's checks first).

**P2, broader research:** Signal and Bitwarden for trust, identity and recovery; libp2p and Syncthing as networking and replication alternatives; Essentia for Soundscape; Maestro and ViewInspector for testing; ElectricSQL and Yjs as other sync models.
