# Local AI in Chat: design and milestones

Status: **approved by Reuben, 2026-11-07** (all recommendations in section 6 accepted). L0 finished on 2026-11-07 (results below); Reuben skipped the remaining checks, which are recorded as skipped, not passed. L1 (the Assistant in Chat on Foundation Models) is built; its phone run is in [Chat's ACCEPTANCE.md](../examples/chat/ACCEPTANCE.md#l1-on-device-assistant).

Goal: a private, offline AI assistant inside the Chat app on the iPhone. It downloads its own model, runs it on the phone and keeps working in airplane mode. It is the flagship's first feature that needs no server. For ixOS it proves a reusable **local model capability**: a native Swift inference adapter behind a stable interface that Rust app code drives, with storage, streaming, cancellation and resource limits handled by the framework.

The architectural rule, from Reuben's proposal, and kept: **no inference in Rust.** Inference engines are native Swift adapters. Rust owns the conversation, the prompt, the history, the settings, the model library's state and the policy (what may load, when to stop). The adapter owns tokens, weights and the hardware.

## 1. What the audit found

### The Chat app today (main at `cd8a31a`, plus draft PR #51 for M2)

| Area | Today | What it means for a local assistant |
|---|---|---|
| App crate | `examples/chat/rust` (`ChatCore`, UniFFI namespace `forge_ffi`), on `chat-core` | The assistant lives in this crate, next to the server chat, not in `chat-core` (which is the server protocol). |
| Account gate | `ContentView` shows `SignUpView` until there is an account; `ChatCore` keeps a `Session` per account | **The assistant must not need an account or a server.** It needs its own state outside `Session`, and the UI must reach it while signed out. |
| Conversations | Server rooms, per-room SSE pumps, client-chosen message ids, `since` cursors | An assistant conversation is local only. It must not go through `ChatRepo`/`HttpRepo` or the outbox. |
| Storage | M1: JSON snapshot. **M2 (PR #51, draft):** `forge-storage::Database` (rusqlite bundled, migrations, FTS5) and `forge-chat.db` per account | Assistant history goes in its own SQLite file (`assistant.db`), not per account, so it survives sign-out. Depends on M2 merging. |
| Events | One `EventHub<ChatEvent>`, queue 512 per consumer, `EventsDropped` on overflow | Token streaming at 20–50 tokens/s would flood a shared queue. Use a **separate per-generation stream** (below), and send coalesced text, not one event per token. |
| Async and cancellation | `forge_runtime::background` and `background_cancellable` with `CancelHandle`; UniFFI Swift does **not** cancel Rust futures when a `Task` is cancelled (async bridge finding) | Cancellation must be an explicit command, as Chat already does with `unsubscribe`. |
| Native services | `forge_services::native_services!()` with foreign traits (`#[uniffi::export(with_foreign)]`, async via `async_trait`), Swift adapters in `crates/forge-services/swift`, `NativeServices::for_tests` fakes | This is exactly the shape for a `LocalModelEngine` foreign trait, with a fake engine for Rust tests on Linux. |
| Lifecycle | `set_phase`; in the background every stream stops | Generation must stop (or finish quickly) on background: iOS kills GPU/ANE work from suspended apps, and the 1–3 GB model should be unloaded under memory pressure. |
| App config | `forge.toml` writes Info.plist, entitlements, `xtool.yml`; `team_type = "free"` refuses paid entitlements | Paid-only entitlements (below) are unavailable, so the design must fit the default memory limit. |
| Deployment target | Chat is `minimum_ios = "17.0"` | Foundation Models needs iOS 26, CoreML-LLM iOS 18. See open question Q3. |
| Builds | Linux with xtool (Xcode 26 SDK, Swift 6.2.3); Swift only compiled by macOS CI in the cloud | No `.metal`, `.mlmodel` or `.mlpackage` may need compiling at build time. Swift macros from Xcode-only plugins (for example Foundation Models' `@Generable`) are a risk. |
| Ledger | `docs/capability-ledger.md`: "Core ML: No, `.mlmodel` compiles only on a Mac" | Downloading models and compiling them **on the phone** (`MLModel.compileModel(at:)`) sidesteps the Mac. That is the capability to prove. |

### Reuben's claims, checked against the sources

| Claim | Verdict | Source and notes |
|---|---|---|
| CoreML-LLM gives iOS inference for Qwen and Gemma, on Swift Package Index | **Holds** | [john-rocky/CoreML-LLM](https://github.com/john-rocky/CoreML-LLM), MIT, v1.9.0, last commit 2026-10-05, [listed on SPI](https://swiftpackageindex.com/john-rocky/CoreML-LLM). One library product, `CoreMLLLM`; depends on `huggingface/swift-transformers`. Core ML on the Neural Engine, not MLX. |
| Qwen3.5 0.8B ≈ 1.2 GB | **Holds** | README table: 1.2 GB, `mlboydaisuke/qwen3.5-0.8B-CoreML`. The base model is listed as 0.9B parameters. |
| Qwen3.5 2B ≈ 2.8 GB | **Holds** (size) | README: 2.8 GB. Whether it fits the free-team memory limit on an 8 GB phone is unknown (below). |
| LFM2.5 350M ≈ 810 MB | **Holds** (size) | README: 810 MB. **License caveat:** LFM Open License v1.0, which does not license commercial use by entities with $10M+ annual revenue. Fine for a demo, worth knowing for a framework default. |
| Speed figures are from an iPhone 17 Pro | **Holds, but the figures conflict** | README: "iPhone 17 Pro A19 Pro, 2048-token context, ANE-only": 0.8B ~48 tok/s, 2B ~27, LFM2.5 52. The 0.8B Hugging Face card says 22.1 decode tok/s and mentions an iOS 26.1 Core ML runtime bug. Cati (A17 Pro, iOS 26.3.1) will be slower than either; only our own measurement counts. |
| Qwen licensing | **Better than assumed** | Qwen3.5 0.8B and 2B are Apache-2.0. |
| CoreML-LLM is a drop-in streaming engine | **Partly** | The general API `stream(...)` is an `AsyncStream<String>` with real cancellation. But Qwen3.5 runs through a separate class, `Qwen35MLKVGenerator`, with a callback API (`generate(inputIds:maxNewTokens:onToken:)`) and no cancellation check in that file (read, not run). Our adapter must handle cancellation itself (stop at the next token callback) until a device test proves otherwise. |
| Models download at runtime and work offline | **Holds** | Pre-converted models on Hugging Face; the package downloads them with a background `URLSession` (pause and resume) and compiles `.mlpackage` on the device. Qwen3.5 0.8B ships only `.mlpackage`, so the first load compiles: expect a slow first load and roughly double the disk space during it. |
| CoreML-LLM builds on Linux with xtool | **Plausible, unproven** | The repo has no `.metal`, `.mlmodel` or `.mlpackage` files. swift-transformers and its dependencies have never been built with xtool here. Spike L0. |
| CoreML-LLM's minimum iOS | **iOS 18** | `platforms: .iOS(.v18)`. Chat is iOS 17 today. |
| Apple Foundation Models: on-device, offline, no weights to ship | **Mostly holds** | iOS 26.0+. iPhone 15 Pro/Pro Max qualifies, so Cati does. `SystemLanguageModel.default.availability` reports `.deviceNotEligible`, `.appleIntelligenceNotEnabled` or `.modelNotReady`: **Apple Intelligence must be turned on in Settings** and the region/language must support it. Context is 4096 tokens; `streamResponse` yields cumulative snapshots, not deltas. No entitlement is documented, but **we found no primary source on free personal-team apps**: spike L0 settles it. |
| An alternative: MLX Swift | **Poor fit for Linux** | MLX's own README says SwiftPM cannot build its Metal shaders; the final build needs Xcode. |
| An alternative: llama.cpp | **Workable fallback** | Prebuilt XCFramework as a SwiftPM binary target, Metal library embedded, so nothing compiles on Linux. Untested with xtool. Keep it as the second adapter if CoreML-LLM fails L0. |

### Device and account constraints

| Constraint | Finding | Design response |
|---|---|---|
| Memory | Cati has 8 GB. Apple publishes no per-app limit. `com.apple.developer.kernel.extended-virtual-addressing` is **paid only** in Apple's capability table; `increased-memory-limit` is not in the table and is probably paid only too. Unverified for free teams. | Design for the **default limit**. The adapter reads `os_proc_available_memory()` before loading and refuses a model that would not fit with headroom, with a clear message. L0 records the real number on Cati. The 2B model is "if it fits", not a promise. |
| Downloads | Background `URLSession` needs no entitlement. Apple's Background Assets needs the paid program and TestFlight/App Store. | Background `URLSession` (as CoreML-LLM does), resumable, into Application Support marked excluded from backup. Show free space before starting. |
| Free team | 3 sideloaded apps; Cati now holds Pocket Library, ServicesCheck and Weather. Chat was removed at 13:50. | L0's check app and Chat each need a slot. See Q5. |
| Background | iOS suspends the app within seconds; GPU/ANE work in the background is not allowed. | Generation stops on background with a "Stopped: app left the foreground" state. No background inference. Downloads continue through the background session. |
| Thermal and battery | `ProcessInfo.thermalState`, `UIDevice` battery level and state, Low Power Mode are all public, no entitlement. | Shown in the resource monitor; at `.serious` the app warns, at `.critical` it stops generation. |

## 2. The design

### Layers

```
SwiftUI (Assistant tab, model library, controls, monitor)
        │  UniFFI: AssistantCore commands + per-generation token stream
Rust: examples/chat/rust::assistant      ← conversation, prompt, history, settings,
        │                                   policy, timings, storage (assistant.db)
        │  foreign trait LocalModelEngine (forge-ai, later framework)
Swift adapters:  FoundationModelsEngine   CoreMLLLMEngine   (later: LlamaCppEngine)
```

### The provider split

Chat gains a second **provider** beside the server:

- **Server provider** (today): rooms, accounts, SSE, outbox. Unchanged.
- **Local model provider** (new): one or more assistant conversations, stored on the phone, answered by a local engine. No account, no network after the model is downloaded.

They share the UI vocabulary (conversation list, bubbles, composer, search) but not the engine. In Rust, the assistant is a separate object, `AssistantCore`, owned by `ChatCore` but alive without a `Session`. Assistant conversations appear in the Chats list in their own section, and the Assistant is reachable from the signed-out screen too, so a phone that never met the chat server can still use it.

### The Rust-facing engine interface

A foreign trait implemented in Swift, following the `forge-services` pattern so it gets the same install step, version check and fakes:

```rust
#[uniffi::export(with_foreign)]
#[async_trait::async_trait]
pub trait LocalModelEngine: Send + Sync {
    /// Which engine this is and what it can run on this phone right now.
    fn describe(&self) -> EngineInfo;               // id, name, availability, reason
    /// Models this engine knows about (catalogue plus what is on disk).
    async fn models(&self) -> Vec<ModelInfo>;        // id, size, license, state, path
    async fn download(&self, model: String, progress: Arc<DownloadSink>) -> Result<(), AiError>;
    async fn remove(&self, model: String) -> Result<(), AiError>;
    async fn load(&self, model: String, options: LoadOptions) -> Result<LoadReport, AiError>;
    async fn unload(&self) -> Result<(), AiError>;
    /// Streams text into `sink` until done, the limit, or `sink` says stop.
    async fn generate(&self, request: GenerateRequest, sink: Arc<TokenSink>) -> Result<GenerateReport, AiError>;
    fn resources(&self) -> ResourceSnapshot;          // footprint, available memory, thermal, battery
}
```

- `GenerateRequest` carries the **already-rendered messages** (system, user, assistant turns), the sampling settings (temperature, top-p, max new tokens) and the context limit. Rust builds and trims the history; the adapter applies the model's chat template (it knows the tokenizer) and reports `prompt_tokens`.
- `TokenSink` is a **Rust object** (`#[derive(uniffi::Object)]`) the adapter calls with text pieces. Its `push(text) -> bool` returns `false` once Rust has cancelled, so the adapter stops at the next token even if the engine itself has no cancel check (the Qwen3.5 case). The adapter also cancels its own `Task`.
- Foundation Models yields cumulative snapshots; its adapter sends only the new suffix, so Rust always receives deltas.
- `GenerateReport` and `LoadReport` carry the timings for the monitor: load time (and whether it compiled), time to first token, prompt and output token counts, tokens per second, peak footprint, stop reason (`Finished`, `Length`, `Cancelled`, `Background`, `Thermal`, `MemoryWarning`, `Error`).
- `AiError`: `Unavailable { reason }` (with Foundation Models' reasons mapped), `NotDownloaded`, `InsufficientMemory { needed_mb, available_mb }`, `InsufficientStorage`, `DownloadFailed`, `ContextTooLong`, `Engine { reason }`.

Engines are installed at launch like native services (`LocalModels.install([FoundationModelsEngine(), CoreMLLLMEngine()])`). Rust picks by `engine_id/model_id`. A fake engine (scripted text, delays, failures) lets every Rust behaviour be tested with `cargo test` on Linux.

**Where it lives.** First in the Chat app crate (`examples/chat/rust/src/assistant`), because the capability rule is "build only what the next demo needs". The trait and records move into a framework crate (`forge-ai`, a macro like `native_services!` so the items join the app's UniFFI namespace) and the Swift adapters into `crates/forge-ai/swift` once the design survived the device runs (milestone L5).

### Streaming to SwiftUI

`AssistantCore::send(conversation_id, text)` returns at once with a `Generation` object. The host pulls from `Generation::next() -> Option<AssistantDelta>` (async, like Chat's event subscription) and wraps it in an `AsyncSequence`. Rust coalesces pushes into deltas of at most ~50 ms, so the UI updates about 20 times a second however fast the engine is. The shared `ChatEvent` stream only gets the start and the end (`AssistantMessageUpdated`), so the token rate never hits its 512-event queue.

`Generation::cancel()` is the explicit stop (the Stop button, a new message, leaving the screen). The partial answer is kept and marked "Stopped".

### Persistence

`assistant.db` in Application Support, through `forge-storage::Database` from M2: `conversations`, `messages` (role, text, model id, settings snapshot, stop reason, timings), `models` (what is downloaded, bytes, last used) and `settings`. FTS5 over messages so Chat's search covers assistant chats. A generation in progress is written as `Streaming` and becomes `Stopped (interrupted)` at the next launch if the app was killed. Model files live under `Application Support/Models/<engine>/<model>/`, excluded from backup; Rust stores only relative paths.

### Controls

Per conversation, with app-wide defaults: model, temperature (0–1.5), top-p, maximum answer length (tokens), context budget (tokens; capped by the model: 4096 for Foundation Models, 2048 as CoreML-LLM's published config), and a system prompt. Rust trims the oldest turns to fit the budget and shows when it did.

### Characters

Reuben's addition (2026-11-07): the user can pick or create a **character** for the assistant to play. A character is a harness around the model, not a different model:

- **The record**, in `assistant.db`: name, an emoji or initials for the avatar, a one-line description, the character's instructions (personality, voice, what it knows, what it won't do), an optional opening line, and optional defaults (model, temperature). The app ships two built-in test characters, a virtual boyfriend and a virtual girlfriend (Reuben, 2026-11-07), plus the plain Assistant with no character. The user can make, edit, duplicate and delete their own.
- **Each conversation is bound to one character**, chosen when it starts. The list shows the character's avatar and name, and its bubbles carry them.
- **Rust builds the harness prompt**: a short fixed frame (stay in character, answer as the character, keep it safe), then the character's instructions, then the trimmed history. The character block is **pinned**: history trimming never drops it, and the context budget reserves room for it.
- **Small models drift out of character** after a few turns. The harness re-states a one-line character reminder just before the newest user message, and L0 measures how well 0.8B holds a character over ten turns with and without it.
- **Foundation Models applies Apple's own guardrails** on top, so some characters may get refusals there. The UI shows the refusal as a refusal, not as an error.
- Characters are plain data, so a later step can import and export them as small JSON files through the share sheet.

All of it is Rust and SQLite, with nothing engine-specific, so it works the same on every adapter and moves into `forge-ai` with the rest in L5.

### Resource monitor

A Settings → Assistant → Diagnostics screen, and a compact line under each answer: model, load time (with "compiled on first load"), time to first token, tokens per second, prompt/answer tokens, app footprint (`task_vm_info.phys_footprint`), `os_proc_available_memory()`, thermal state, battery level and Low Power Mode. Every generation's report is logged through `forge_runtime::logging`, so `forge device logs --grep assistant` on Linux records the numbers without anyone copying them by hand. Memory warnings unload the model.

### The airplane-mode test

Part of every device runbook from L2 on: download a model with network, force-quit, turn on airplane mode (Wi-Fi and cellular off), relaunch, start a new assistant chat, get a streamed answer, cancel one, relaunch and find the history. Proof that nothing reaches the network: the app's Rust `forge-net` logs every request, the Assistant screen shows "0 network requests since launch" from that counter, and the ledger records the run.

## 3. Milestones

Chat's sequence today: M0 and M1 done, M2 is draft PR #51, M3 (attachments) is being built on the M2 branch, then M4 voice, M5 notifications/deep links/background, M6 device acceptance. The local AI work is a track **L0–L5**. L0 touches no Chat code, so it can start now. L1 onwards depends on M2 merging (SQLite) and should start after M3 merges, so two threads never edit `examples/chat/rust/src/lib.rs` and `ChatModel.swift` at once. Proposed order: **M3 → L1–L4 → M4 → M5 → L5 → M6**, with M6's acceptance run covering the assistant too.

| Milestone | What | Done when (on Cati, recorded by Reuben) |
|---|---|---|
| **L0 spikes** (no Chat code) | A throwaway check app, like ServicesCheck, built on Linux with xtool: (a) Foundation Models availability and a streamed answer, signed with the free team; (b) CoreML-LLM + swift-transformers build with xtool; download Qwen3.5 0.8B, compile on device, generate, cancel mid-answer (**acceptance: Stop must end the computation itself, not just hide the text**, within one decode step); (c) `os_proc_available_memory()` at launch and after load; (d) the Swift-to-Rust `TokenSink` round trip at full token rate | A short results table in this doc: what built, load/compile time, tokens/s, footprint, available memory. Decides whether the 2B model is offered and whether llama.cpp is needed. |
| **L1 Assistant core and baseline** | `AssistantCore` in the Chat crate, the `LocalModelEngine` trait, a fake engine and Rust tests, `assistant.db`, the Foundation Models adapter, an Assistant conversation in the UI reachable signed in or out, streaming and Stop | A Foundation Models conversation works, Stop works, history survives a relaunch. If Foundation Models is unavailable, the screen says why (Apple Intelligence off, not ready). |
| **L2 Downloadable models** | Model library (catalogue, download with progress and resume, remove, switch, free space and memory checks), the CoreML-LLM adapter, Qwen3.5 0.8B default, LFM2.5 350M as the small option, 2B only if L0 says it fits | Download, switch and remove each model; the **airplane-mode test** passes. |
| **L3 Controls, characters and limits** | Temperature, top-p, answer length, context budget, system prompt; characters (built-in and user-made, pinned in the prompt); history trimming; background, thermal and memory-warning stops | Each control visibly changes behaviour; a user-made character stays in character for ten turns on 0.8B; backgrounding mid-answer stops cleanly; a memory warning unloads. |
| **L4 Resource monitor** | Diagnostics screen, per-answer stats line, logged reports, performance rows added to the ledger | A table of measured load time, first-token time, tokens/s and footprint for each model on Cati, in the ledger. |
| **L5 Framework extraction** | Move the trait, records and Swift adapters into `forge-ai` with `install`, version check and fakes; `docs/local-ai.md` becomes the developer guide; ledger rows | A `forge init` app can add local AI by following the doc (part of the "build without the maintainer" exercise). |

### L0 results

The check app and its runbook: `scripts/make-ai-check-app.sh`, [local-ai-check.md](local-ai-check.md). Found while building it, before any device run:

- CoreML-LLM keeps models in `Documents/Models`, which iCloud backs up. The check app marks the folder excluded from backup; the L2 adapter must do the same or move it.
- `Qwen35MLKVGenerator` compiles each `.mlpackage` chunk into a temporary location on every load (`MLModel.compileModel(at:)` without caching the result), so every launch may pay the compile time. K7 measures it; L2 may need to keep the compiled `.mlmodelc`.
- Cati is a 512 GB phone with about 15 GB shown free in Settings, yet the app saw only 1.3 GB (`volumeAvailableCapacityForImportantUsage`) when the 2B retry failed. The model library must check what the app can actually use, ask for download plus compile headroom up front, and tell the user how much to free, not rely on the Settings figure.
- Loading needs free disk for the compiled copy too (about 2 GB for the 2B model), and a failed compile leaves it in `tmp/`. L2's model library must compile once into Application Support, reuse it, clean up after failures, and check free space for download plus compile before starting.
- Qwen3.5's tokenizer comes from Hugging Face at load time (`AutoTokenizer.from(pretrained:)`), not with the model download. K12 tests whether its cache makes it work offline.
- CoreML-LLM's own sample app drops system messages for Qwen3.5. The check app sends them through the chat template; K9 shows whether that works for characters.
- Reuben's review (2026-11-07): stopping must be real. The first check app only ignored Qwen3.5's tokens after Stop while the generator kept running; it now builds a patched copy of the generator (CoreML-LLM 1.9.0, MIT, checksum pinned) that stops before the next prefill or decode step. LFM2.5 stops through CoreML-LLM's own `AsyncStream` cancellation, and Foundation Models when its stream is dropped.
- If swift-transformers cannot render Qwen3.5's chat template, the check app no longer falls back to the last user message alone (which dropped the character and the history): it writes the same ChatML by hand with the whole conversation and logs that it did.

| Question | Result on Cati |
|---|---|
| Foundation Models on a free-team app | **Available** (K2, 2026-11-07); generation **skipped by Reuben** (2026-11-07: "it works") |
| CoreML-LLM builds with xtool on Linux | **Yes**: CoreML-LLM 1.9.0 + swift-transformers, debug, 162 s, no errors (Reuben's laptop, xtool 1.24.0) |
| Available memory at launch / after 0.8B load | **3,350 MB** at launch (footprint 22 MB), 3,158 MB after loading Qwen3.5 0.8B (footprint 217 MB). The Core ML weights mostly do not count against the app's footprint |
| LFM2.5 350M: load (first, second), tokens/s, stop latency | Download cancelled from the app (`CancellationError`, only thrown by CoreML-LLM's `cancelDownload()`); retry **skipped by Reuben** (2026-11-07: "it works") |
| Qwen3.5 0.8B: load (first, second), tokens/s, stop latency | Debug build: first load 23.4 s, about 9 pieces/s. **Release build (7b8eb70): first load 16.6 s (compiles; ~185 MB of available memory), about 36 pieces/s**, 0 late pieces. Time to first text grows with the conversation (4.0 s, 5.3 s, 6.8 s over three character turns) because Qwen3.5 prefills one step per prompt token and every turn re-sends the whole history: L1 should keep the model state between turns instead of re-prefilling. Stop latency and second load **skipped by Reuben** (2026-11-07: "it works"): real stopping is in the code (patched generator) but unmeasured on the phone |
| Qwen3.5 2B fits | **Failed on disk, not memory** (release build, 2026-11-07): with both Qwen models installed the phone had 2.7 GB free; compiling the 2B's chunks into `tmp/` used 2 GB and `chunk_d` failed ("Failed to build the model execution plan… error code: -14") at 687 MB free. Memory was fine (3,320 MB available, footprint 55 MB). Retry with about 5 GB free. Retry on the fixed build **skipped by Reuben** (2026-11-07: "it works") |
| Characters in character over ten turns (reminder on / off) | Mia, Qwen3.5 0.8B, reminder on: in character for the first three turns ("pretty good", Reuben); ten turns, Leo and reminder off **skipped by Reuben** (2026-11-07: "it works") |
| Offline after download | **skipped by Reuben** (2026-11-07: "it works") |
| Swift → Rust push cost | **1.6 µs per push** in debug, **0.6 µs** in release (10,000 pushes): negligible against ~28 ms per token |

Each milestone is one PR with a device runbook in `examples/chat/ACCEPTANCE.md`, the same as Chat's others.

### L1 as built

The Assistant in Chat, as the L1 row describes, with these choices and differences from section 2:

- **The engine interface starts small.** `LocalModelEngine` has `describe()` and `generate(request, sink)` only; `models`, `download`, `remove`, `load`, `unload` and `resources` arrive with L2's model library and L4's monitor. `generate` returns nothing: Rust's sink measures first-text time, duration and pieces itself, so every engine is timed the same way.
- **Rust runs the engine on a thread of its own** (`forge_runtime::block_on` over the foreign future), so `send` returns at once and the main thread never waits for the model.
- **One answer at a time,** across conversations (`Busy`). Deleting a conversation or shutting down stops its answer.
- **Coalescing:** `next()` hands over what arrived since the last delivery, at most every 50 ms; `catch_up()` lets a screen that opens mid-answer start from the text so far. Assistant traffic does not use the 512-event `ChatEvent` queue.
- **What is saved:** the user's message and an empty `Streaming` answer before the engine starts, the answer's text, state (Done, Stopped, Refused, Failed) and timings when it ends. An app killed mid-answer leaves it `Interrupted` at the next launch, without the partial text. Stopped and interrupted answers stay in the history the model reads; refused and failed ones do not.
- **Characters:** the built-in Assistant, Leo and Mia, from L0, with an opening line each. The pinned frame, the per-turn reminder and oldest-first trimming are L0's harness; Rust fits history into the engine's context at 3 characters per token after room for a 400-token answer. User-made characters and the controls are L3.
- **Foundation Models:** a fresh `LanguageModelSession` per answer, built from a `Transcript`: the character as instructions, each earlier message as a prompt or response entry, and the newest message as the prompt. Only the new part of each snapshot is pushed. The first build flattened the history into one "User: … You: …" prompt; on Cati (2026-11-08) Mia then wrote both sides of the conversation, and the invented turns, fed back, led to "I can't continue this roleplay". Besides the transcript, Rust now ends an answer at a later line that opens a new turn ("User:", "You:", "Me:", "Assistant:" or the character's name), tells the engine to stop, and strips such lines from saved answers before the model reads them again; the character frame talks about texting in a messaging app instead of "role-play". Guardrail refusals become `Refused`, shown as a refusal. iOS 26 and Apple Intelligence are checked on every send and whenever the screen appears or the app returns to the foreground, with the reason shown (Apple Intelligence off, model still downloading, iPhone not eligible).
- **Chat stays on iOS 17.** Foundation Models is gated with `#available(iOS 26, *)`, so L1 needs no minimum change; decision 3 (iOS 18) lands with CoreML-LLM in L2.
- **Not yet:** keeping model state between turns (an L2 concern for Qwen3.5, whose time to first text grows with the history), search over assistant chats, and backgrounding mid-answer (L3).

Tests: 19 Rust tests with a fake engine (`examples/chat/rust/src/assistant/tests.rs`, plus `the_assistant_works_signed_out_and_survives_sign_out`) and Swift model tests with a scripted Swift engine (`AssistantModelTests`, macOS CI).

### Capability ledger rows this adds

To add under a new "On-device AI" section, all **No** today: Apple Foundation Models text generation; Downloaded Core ML model (download, on-device compile, load); Streaming generation into Rust with cancellation; Model library (download, resume, remove, switch); Memory and thermal guard; Offline operation proven in airplane mode. Performance rows: model load time, time to first token, decode tokens/s, peak footprint, per model.

## 4. Risks

| Risk | Likelihood | Fallback |
|---|---|---|
| Free-team apps can't use Foundation Models | Low (no entitlement documented), unverified | CoreML-LLM only; L1 baseline uses the fake engine plus CoreML-LLM |
| swift-transformers or CoreML-LLM doesn't build with xtool | Medium | llama.cpp XCFramework adapter; or vendor only CoreML-LLM's Qwen path |
| The default memory limit is too low for the 2B model | Medium | Offer 0.8B and 350M only; record the measured limit |
| Qwen3.5 path can't be cancelled | **Confirmed in code**: `Qwen35MLKVGenerator`'s decode loop never checks for cancellation, so ignoring its callback leaves the phone computing | The check app builds a patched copy with a stop check before every step (`StoppableQwen35Generator`); L2 either keeps that patch, gets it upstream (with Reuben's go-ahead), or switches engine |
| First-load compile is slow (minutes) or needs too much disk | Medium | Show "Preparing model (one time)" with progress; check free space for twice the model size |
| iOS 26.x Core ML runtime bugs (the HF card mentions one in 26.1) | Medium | Pin compute units to CPU+ANE as the package recommends; record iOS version with each benchmark |
| Raising Chat's minimum iOS breaks nothing today but narrows the demo | Low | Gate the Assistant with `#available` instead (Q3) |
| Foundation Models' `@Generable` macro needs an Xcode-only compiler plugin | Medium on Linux | Use plain text `respond`/`streamResponse` only; no guided generation |

## 5. Recommendation

Build it, as Reuben proposed, in the Chat app: Foundation Models as the quick baseline, CoreML-LLM with **Qwen3.5 0.8B** as the real feature, behind a Rust-facing `LocalModelEngine` foreign trait with no inference in Rust. Start with the L0 spikes now, since they touch no Chat code and settle the four unknowns that could change the plan (free-team Foundation Models, the xtool build, the memory limit, cancellation). Begin L1 once M3 merges. Treat the 2B model as conditional on L0's memory measurement, and treat every published speed number as a guess until Cati has been measured.

## 6. Decisions (Reuben, 2026-11-07)

All recommendations accepted:

1. **Order:** M3 → L1–L4 → M4 → M5 → L5 → M6.
2. **L0 starts now**, in parallel with the Chat thread's M3, without touching Chat code.
3. **Chat's minimum iOS goes up to 18**; Foundation Models is gated with `#available(iOS 26, *)`.
4. **Models:** Qwen3.5 0.8B is the default, LFM2.5 350M the small option, and 2B only if L0 shows it fits.
5. **App slot:** the L0 check app replaces ServicesCheck on Cati; Chat takes a slot again from L1.
6. **The assistant works without a chat-server account.**
7. **Model downloads are Wi-Fi only by default**, with an override.
8. **Built-in characters:** a virtual boyfriend and a virtual girlfriend, as test characters. User-made characters become shareable files later.
