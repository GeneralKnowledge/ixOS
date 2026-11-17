# Forge AI: the on-device AI module and AI over Relay

Status: **approved by Reuben** (2026-11-14: start A0 and A1 with the recommended answers to the open questions). A0 and A1 are built; see the milestones.

From 2026-11-17 the AI Lab follows the [eSIM and AI Lab focus plan](research/esim-ai-lab-plan.md): Apple's model first, then a GTX 1060 laptop over the tailnet. The Relay steps here (A2's `RelayModel`, A4) are deferred.

Reuben (2026-11-14): "We might have to split out the AI into its own thing to fully develop it", "a cool experiment could have the AI communicate via relay", "it could combine AI power over more powerful AIs. We could use OpenAI style input / output for compatibility."

This plan does two things:

1. **Forge AI**, a framework module that every app uses to talk to a language model: one request shape, one Swift adapter for Apple's on-device model, the checks that make a small model's answers usable, and a router that picks where a request runs. It consolidates what Chat ([local-ai.md](local-ai.md), L1) and Pocket Library (#181) each built on their own.
2. **AI over Relay**: the phone sends a request to a bigger model on a nearby machine over Relay's encrypted pairing, and can answer other devices' requests with Apple's model. The request is OpenAI chat-completions shaped, so the laptop end can be llama.cpp, Ollama or LM Studio unchanged.

"Its own thing" here means its own module, track, playground and thread inside ixOS. A separate repository waits for the product split Reuben parked ([roadmap](roadmap.md)); see open question 1.

## 1. What exists today

| | Chat (L1) | Pocket Library (#181) |
|---|---|---|
| Rust trait | `LocalModelEngine` (`examples/chat/rust/src/assistant/engine.rs`) | `SummaryModel` (`examples/pocket-library/rust/src/summary.rs`) |
| Request | instructions + turns (user/assistant) + temperature + max tokens | instructions + one prompt + temperature + max tokens |
| Sink | `TokenSink.push(delta) -> bool` | `SummarySink.update(text_so_far) -> bool` |
| Swift adapter | `FoundationModelsEngine.swift`: transcript entries, default guardrails | `AppleIntelligenceSummarizer.swift`: one prompt, `permissiveContentTransformations` |
| Errors mapped | guardrail, context window | guardrail, **refusal**, context window, **rateLimited → Busy**, **assetsUnavailable** |
| Rust around it | characters, history trimming, invented-turn cutting, `assistant.db` | parts of ≤900 tokens, reader → writer → editor, cast keeper, acts, three overview writers, refusal/preamble/echo/cut-off checker, saved progress per chapter |
| Swift around it | | `BookBackgroundTask` (`BGContinuedProcessingTask`, iOS 26) |

The two adapters are the same wrapper written twice, and each fixed problems the other still has: Chat's misses `refusal`, `rateLimited` and `assetsUnavailable`; Pocket Library's has no transcript support. Good Dog's planned GD7 "Ask" ([good-dog-demo.md](../examples/good-dog/docs/good-dog-demo.md)) would write a third. That is the extraction signal ([architecture.md](architecture.md#framework-principles), rule 2).

## 2. Forge AI, the module

### What moves and what stays

| Moves into Forge AI | Stays in the app |
|---|---|
| The request, answer and error records; the `LanguageModel` foreign trait; the sink; a scripted fake model for `cargo test` | Chat's characters, history, `assistant.db`, invented-turn rules |
| One Swift adapter for Foundation Models: transcript or single prompt, guardrail choice, every error mapped, availability reasons | Pocket Library's roles and prompts (reader, writer, editor, cast keeper, acts, overview), `StoredBookRun` |
| The answer checker: `clean`, declines in words, prompt echoes, cut-off sentences, copied runs; `estimate_tokens`, `split_into_parts` | Good Dog's grounding in approved answers and its welfare rules |
| The router (section 4) and the timings log (`forge_runtime::logging`, never the text) | Which jobs an app marks large, what it shows the user |
| `BGContinuedProcessingTask` wrapper (Swift, platform-level, so it may move on first use) into `forge-services`' `background` adapter | |

The whole-book pipeline itself (parts → notes → combine → final) stays in Pocket Library until a second app needs it. Commonplace article summaries are the likely second use; when that comes, the generic map-and-combine step moves and the book roles stay.

### Where it lives

A Cargo feature, `ai`, of `forge-services`, installed with `native_services!(ai)`, the same shape as `transfers`: Rust in `crates/forge-services/src/ai/`, the Swift adapter in `swift/optional/ForgeServices+LanguageModel.swift`. Nothing goes into `forge-runtime` or `forge-app`. It becomes its own crate (`forge-ai`, as the ledger names it) only if downloadable models (L2, CoreML-LLM) arrive, since that brings a Swift package an app must be able to leave out.

This is a native service with a Rust interface, which the rules allow because Rust reads the answers: it plans the requests, checks them and stores them.

### One request shape (OpenAI chat completions)

```rust
pub struct ChatRequest {            // POST /v1/chat/completions, the subset
    pub model: Option<String>,      // None: the backend's default
    pub messages: Vec<ChatMessage>, // role: System | User | Assistant, content: String
    pub max_tokens: u32,
    pub temperature: f64,
}
pub struct ChatAnswer {
    pub text: String,
    pub finish: Finish,             // Stop | Length | ContentFilter | Cancelled
    pub usage: Option<Usage>,       // prompt and completion tokens, when known
    pub answered_by: ModelInfo,     // which model, on which device
}
```

Text only: no tools, images or JSON mode at first. Chat's instructions-plus-turns and Pocket Library's instructions-plus-prompt both map onto `messages` without loss. The Swift adapter turns `messages` into a Foundation Models transcript; the HTTP upstream sends it as JSON; Relay sends it as CBOR. An Apple refusal becomes `ContentFilter`, as OpenAI reports one.

```rust
#[uniffi::export(with_foreign)]
#[async_trait::async_trait]
pub trait LanguageModel: Send + Sync {
    fn describe(&self) -> ModelInfo;   // id, name, where, available + reason, context tokens
    async fn generate(&self, request: ChatRequest, sink: Arc<TextSink>) -> Result<(), AiError>;
}
```

`TextSink.push(delta) -> bool` (Chat's model: deltas, false means stop now). Rust keeps the text so far, so Pocket Library's live draft comes from Rust, not from the adapter. `AiError` is the union of both today: `Unavailable`, `Refused`, `ContextTooLong`, `Busy` (throttled in the background), `Stopped`, `Engine`.

Pocket Library's constants become functions of the model's `context_tokens` (parts of about a fifth of the window, not a fixed 900), so a bigger model on the laptop reads a chapter in fewer, better requests with no other change.

### The playground in Device Lab

An **AI Lab** entry under Device Lab's Apps: pick where it runs (this iPhone, a paired device), write or paste a prompt or a long text, watch the answer stream, and see what the checker removed, time to first text, pieces per second, and why a model is unavailable. Every later step is checked here first, before the apps.

## 3. AI over Relay

Relay (#182, #192) already pairs devices by QR code, discovers them with Bonjour and talks over a Noise channel with CBOR messages. Its SwiftUI app runs in Device Lab; receiving a file with a word code passed on an iPhone on 2026-11-15.

### The protocol

A new kind of exchange beside notes and files. Protocol version 2; a version 1 peer declines it.

```
asker → Ask { id, request: ChatRequest }
server → Delta { id, text } …            (as the model writes)
server → Answered { id, finish, usage, model } | Failed { id, error }
asker → Stop { id }                       (at any time; the server stops the model)
```

`Hello` gains what the device can answer (`serves: [ModelInfo]`), so the asker knows a bigger model is there before sending. Only paired devices can ask. One request runs at a time per server; others wait or get `Busy`. Relay's history records each exchange as a new item kind (`Ask`: model, device, tokens, duration), without the text unless Reuben wants it kept (question 3); the asking app keeps the text, as it does now.

### The laptop end

```bash
relay serve-ai --upstream http://127.0.0.1:11434/v1      # Ollama
relay serve-ai --upstream http://127.0.0.1:8080/v1 --model qwen3-8b   # llama.cpp server, LM Studio
relay ask <peer> "Summarise this" [--model …]              # test from a terminal
```

`serve-ai` accepts Relay asks from paired devices and forwards them to any OpenAI-compatible server as `stream: true`, turning its server-sent events back into `Delta`s (`forge-net`'s ureq client already reads SSE for Chat). The upstream stays on localhost and is never exposed to the network; Relay's pairing is the only way in. `relay ask` makes every step testable on Linux with two terminals and no phone. A person needs no agent for any of it.

### The phone as a peer

With "Answer with Apple Intelligence" on, the Relay app on the iPhone serves asks from paired devices with Apple's model, while it is on screen (Foundation Models throttles background apps). From the laptop, `relay ask iphone "…"` gets an answer from the phone, and `relay serve-ai --openai-listen 127.0.0.1:8090 --peer iphone` lets any OpenAI client on the laptop use the phone's model.

### Using it from apps

Inside Device Lab every app shares one process, so a `RelayModel` (implementing `LanguageModel` in Rust over a Relay link) can be offered to Chat, Pocket Library and the AI Lab directly. A standalone Chat or Pocket Library cannot reach the Relay app's pairing (separate sandboxes), so the pairing and link code moves into a framework module once a standalone app needs it; Relay's friction note A16 (extract the Noise channel from `forge-devproto`) is the first half of that. Until then, AI over Relay is a Device Lab feature.

## 4. Routing

Rust decides where each request runs; Swift never does.

- **Default: on the device.** Apple's model answers when it is available and the request fits its window (4,096 tokens, prompt and answer).
- **A large job goes to a nearby bigger model** when one is paired and reachable: a request that does not fit, or a job the app marks large (a whole-book overview, a long conversation). The app sees which model answered (`answered_by`) and says so ("Answered by qwen3-8b on create").
- **Fallback:** if the peer drops mid-job, the router finishes on the device and the job's saved progress means nothing is redone. If nothing is available, the app gets `Unavailable` with the reason.
- **The user can pin a choice** per app (On this iPhone only, Prefer nearby, Ask each time). Text leaves the phone only to a device the user paired, and only by these rules.

## 5. Milestones

Each is one PR, small enough to review, with the phone part short and batched into a sitting.

| Step | What | Done when | iPhone? |
|---|---|---|---|
| **A0 Forge AI contract** (smallest first step). **Built**: `forge_services::ai` and `native_services!(ai)`, `ForgeAppleModel`; Swift not yet compiled | `forge-services` `ai` feature: records, `LanguageModel`, `TextSink`, fake model, the checker; one Swift adapter merging both; Chat and Pocket Library switch to it with no change in behaviour. Ledger row updated | Both apps' Rust tests pass unchanged on Linux; the laptop build check compiles Swift; fewer lines than today | 3 min: one Mia message, one chapter summary in Device Lab |
| **A1 Relay asks, Linux only**. **Built**: tested on Linux with scripted models and a fake OpenAI server; not yet run against a real Ollama or llama.cpp | Protocol v2 (`Ask`/`Delta`/`Answered`/`Stop`), `relay serve-ai`, `relay ask` | Two terminals on the laptop: `relay ask` streams an answer from Ollama or llama.cpp, Stop stops the upstream | No |
| **A2 AI Lab and phone → laptop** | AI Lab in Device Lab; `RelayModel`; Relay app shows asks in its history | On the phone, AI Lab answers from the laptop's model and from Apple's, with timings | ~5 min |
| **A3 Router and the bigger book** | Router with the rules above; Pocket Library parts sized from the model's window; "answered by" in its summary footer | A whole book summarised on the laptop's model from the phone, and the same book falling back to the phone mid-run | ~10 min |
| **A4 The phone as a peer** | Relay app serves asks with Apple's model; `relay serve-ai --openai-listen` | An OpenAI client on the laptop gets an answer from the phone | ~5 min |
| **A5 Good Dog Ask (GD7)** | Ask on Forge AI, grounded in approved answers, welfare rules in Rust | GD7's own acceptance | Its own |
| **Later** | Pairing extracted for standalone apps; L2 downloadable Qwen (CoreML-LLM) as another `LanguageModel`; `@Generable` structured answers once xtool can build the macro | | |

A0 and A1 touch different code (framework and apps vs `examples/relay`) and can run in parallel. L2 stays parked: with a bigger model reachable at home, a downloaded model mainly buys offline use, which Apple's model already gives for small jobs.

## 6. Risks

- **The laptop is shared and has no GPU that we know of.** An 8B model on CPU writes a few tokens a second, and serving competes with builds (the build lock does not cover it). A2 measures it before A3 depends on it.
- **Foundation Models throttles background use**, so the phone as a peer works only on screen, and a book run in the background still pauses (as today).
- **Relay's app has never run on a phone.** A2 is also its first phone run; R2's own checks come first in that sitting.
- **Different models fail differently.** The checker was tuned on Apple's model; a bigger model may need fewer checks, not more. The AI Lab shows what it removed so this can be seen rather than guessed.

## 7. Open questions for Reuben

1. **"Its own thing":** a module, track and playground inside ixOS, as proposed, or a separate repository now? Recommendation: inside ixOS until the product split happens; nothing here makes moving it later harder.
2. **First step:** A0 (consolidate, then build on one request shape) with A1 in parallel, as proposed, or Relay first?
3. **Relay history:** keep only what was asked of which model (recommended), or the full text of AI exchanges too?
4. **Default routing:** on the device first, the bigger model only for jobs that don't fit or that the app marks large (recommended), or the bigger model whenever it is nearby?
5. **The laptop's model:** which server (Ollama is the easiest to set up; llama.cpp server the lightest) and which model? Does the laptop have a GPU?
6. **Book text leaving the phone:** is "only to a paired device, shown on the summary" enough, or should every large job ask first?
7. **L2 (downloadable Qwen):** stay parked, as recommended?
