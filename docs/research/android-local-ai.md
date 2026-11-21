# On-device AI for Android: findings and a comparison plan

Research, 21 November 2026. Reuben shared a survey of Android projects and inference engines on 2026-11-21 and decided the same day (card in the Android parity thread) that real on-device AI on Android is a **new stage after A8**. Until then, the parity plan's A7 shows Pocket Library's AI features as unavailable on Android. Nothing on this page adds a dependency or code. The projects below are Reuben's findings and have **not yet been checked** here: their licences, upkeep, sizes and claims are what stage R0 checks first.

## Where it fits in ixOS

The interface already exists. `forge-services` has Forge AI's `LanguageModel` foreign trait (`crates/forge-services/src/ai/mod.rs`, plan in [forge-ai-plan.md](../forge-ai-plan.md)):

- `describe()` says which model this is, where it runs, whether it is available and why not, and its context size.
- `generate(request, sink)` streams text into a `TextSink`, and Rust stops it by returning `false` from the sink.

The rule from [local-ai.md](../local-ai.md) holds on both platforms: **no inference in Rust**. Rust owns the request, the history, the prompts, the checks on a small model's answers, and when to stop. The platform engine owns tokens, weights and hardware. On iOS, Apple's Foundation Models implements the trait, and CoreML-LLM with Qwen3.5 0.8B is planned for L2 (llama.cpp is only the fallback there).

So the `LocalAIProvider` Reuben sketched is, in ixOS terms, **a Kotlin class implementing `LanguageModel`**. Loading and unloading become part of the engine and of a model library, as L2 plans for iOS. Cancelling is the sink returning `false`. That keeps everything above the engine shared: Pocket Library's summaries, Chat's assistant and the AI Lab run unchanged on Android. The same shape is how `ForgeHttpTransport` brought Android's HTTP stack to Rust in A4.

What can't be shared is the weights format. iOS's planned engine reads Core ML packages, while llama.cpp reads GGUF and LiteRT-LM reads its own format. The same model (Qwen3.5 0.8B) can come in each format, so prompts and quality checks carry over even though the files don't.

## The findings (Reuben, 2026-11-21, unchecked)

**Apps to learn from**

| Project | What it offers | Why look |
|---|---|---|
| Google AI Edge Gallery | Google's Android app on LiteRT-LM: model downloads, on-device runs, hardware compatibility | Google's current direction for Android inference |
| AiPaca | Kotlin and Compose, llama.cpp (wraps llama-server), GGUF model management, streaming, downloads, a foreground service, benchmarks | The closest to a native Kotlin llama.cpp integration |
| OpenWeights | Kotlin and Compose with llama.cpp and ExecuTorch, Hugging Face discovery, model-fit estimates, optional agent tools | Model discovery and fit estimation |
| OnDeviceAi | LiteRT against llama.cpp on the same hardware | The trade-offs between the two |
| OlliteRT | A phone as a local OpenAI-compatible server on LiteRT-LM | Relevant to AI over Relay, which is already OpenAI-shaped |

**Engines**

| Engine | What it offers | Reuben's view |
|---|---|---|
| llama.cpp | GGUF, CPU and accelerator backends | The best starting point |
| LiteRT-LM | Google's mobile LLM runtime | A strong alternative, especially for hardware acceleration. Use it, not the older MediaPipe LLM Inference API it replaces |
| MLC-LLM | Compiled models for mobile GPUs | Worth a look for speed, more involved |
| ExecuTorch | PyTorch deployment with CPU, GPU and NPU backends | A longer-term alternative |
| MNN | A light engine with mobile LLM support | A benchmark candidate |

**Architecture references**

- **EdgeDroid SDK** treats llama.cpp, ExecuTorch and LiteRT as interchangeable runtime plugins. In ixOS, Forge AI's trait already plays that role, so EdgeDroid is a reference for the Kotlin side only.
- **The Android on-device LLM PoC** compares MediaPipe, LiteRT-LM, llama.cpp and MNN on Pixel hardware. Its method is the model for R1, even if none of its code is used.

His ranking: AiPaca (implementation), EdgeDroid (architecture), AI Edge Gallery (platform), OpenWeights (features), the PoC (benchmarks). His main point: ixOS needs one AI capability shared by its apps, not another chat app.

## The stage, after A8

| Step | Work | Done when |
|---|---|---|
| R0 Check | Read AiPaca, EdgeDroid, AI Edge Gallery and the PoC. For each candidate engine, record its licence, last release, Android ABIs, APK size and whether it builds from Linux with the NDK we already use. GPL code is read only if its licence allows what we would do with it ([android-ports.md](android-ports.md) rules). | A table in this doc; a shortlist of two engines |
| R1 Compare | A throwaway Android check app (like L0 on iOS) runs both shortlisted engines with the same prompts and, where both formats exist, the same model (Qwen3.5 0.8B). It measures load time, time to first text, tokens per second, memory, and how fast Stop really stops. | A results table. The emulator gives functional results only, since it has no Android GPU or NPU; speed numbers count only from a real phone, once Reuben's is back |
| R2 Engine | The chosen engine as a Kotlin `LanguageModel` in Pocket Library's Android app, with a model download in the app. A7's "unavailable" state becomes a real one when no model is downloaded. | Pocket Library's AI summaries run on Android, through the same Rust code as on iOS |

Apps first holds: the engine lives in the Android app until a second Android app needs it, as `ForgeHost` and `ForgeHttpTransport` do.

## Open questions

1. Does iOS change engines? If CoreML-LLM stays the iOS engine, the two platforms share prompts and checks but not weights. If L2 moves to llama.cpp, both could share GGUF files. That is an iOS decision for L2, not something this stage should force.
2. Should the Android engine also serve AI over Relay (OlliteRT's idea)? Only after R2, and only through Relay's own identity and transport.
