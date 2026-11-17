# FluidAudio: a shared local speech capability?

Status: 2026-11-17. Part of the [External Projects Atlas](../../README.md), P0 investigation 2. Research only: nothing is added to ixOS by this page.

## Question

Could FluidAudio give several ixOS apps speech capabilities (transcription, voice activity, speaker separation, speech output) on the iPhone, built with xtool on Linux, without each app building its own?

## Evidence

| Field | Value |
|---|---|
| Date | 2026-11-17 |
| Inspected | [FluidInference/FluidAudio](https://github.com/FluidInference/FluidAudio) at `16780f0` (main, 2026-11-13); latest tag `v0.9.1` |
| Licence | Apache-2.0 for the SDK (`LICENSE`). **Each model has its own licence** on Hugging Face, which has to be checked per model before shipping (for example, NVIDIA's Parakeet models and Liquid AI's LFM models are not under the SDK's licence) |
| Activity | 221 commits since 2026-08-17, several contributors; about 3,300 stars; a long list of shipping iOS and macOS apps in its README |
| Read or run | Source and package manifest read. **Not built** with xtool, **not run** on the iPhone |
| Build evidence | None yet. It imports CoreML, Accelerate and AVFoundation, so it can't build for Linux itself; the check is an xtool iOS build on the laptop (below) |
| Platform evidence | None from ixOS. The README lists iOS apps using it, and its model notes give iOS minimums (iOS 17 for Parakeet v3 and Ultra; iOS 18 for several newer models) |
| ixOS baseline | main at `98a8446`. Speech recognition is item 26 of the [native capability roadmap](../../../../native-capability-roadmap.md) (Apple's Speech framework, Chat voice messages as the likely first user). Speech output is AVSpeechSynthesizer in ForgeReader. Audio recording and playback are in `forge-services` (capability ledger, Audio row) |

## What it does

A Swift package of Core ML audio models, run mostly on the Neural Engine: batch and streaming transcription (Parakeet TDT v3 and its variants, Nemotron streaming, others), end-of-utterance detection, voice activity detection (Silero), speaker diarization (pyannote, Sortformer), several text-to-speech models, and echo and noise suppression. Models are downloaded at run time from Hugging Face as **already compiled `.mlmodelc` bundles** (`ModelNames.swift`), with an offline mode for apps that ship them. A few newer models compile on first use.

### What matters for xtool on Linux

From the manifest and source (not yet from a build):

| Item | Why it matters | Assessment |
|---|---|---|
| Pure Swift plus two small C/C++ targets (fastcluster, a `mach_task_self` wrapper) | xtool compiles C and C++ for iOS with clang | Likely fine |
| One **binary target**, `NemoTextProcessing.xcframework` (a prebuilt Rust static library, about 8 MB per slice) | Binary xcframeworks haven't been tried with xtool in ixOS (local-ai.md says the same of llama.cpp). It would also put a second Rust runtime next to the app's own | Avoidable: `Package@swift-6.2.swift` makes it an opt-out **trait**, and its own comment names "the app links its own Rust runtime" as the reason to turn it off. An ASR-only build uses `traits: []` |
| `TTS` and `Diarizer` are traits too | Smaller builds when only transcription is wanted | Fine on Swift 6.2, which xtool uses |
| A resource bundle (`.process("Resources")`) for text-to-speech | Resource processing differs between Xcode and SwiftPM | Avoided by leaving the `TTS` trait off |
| No `.metal` files, no `.mlmodel` or `.mlpackage` in the package | These are what make MLX and some Core ML routes need a Mac | Good: models arrive compiled, unlike CoreML-LLM's Qwen path in [local-ai.md](../../../../local-ai.md) |
| `import CoreAI` (iOS 27) | Would fail on older SDKs | Guarded by `#if canImport(CoreAI)` |

## Compared with ixOS

ixOS's rule for native capabilities (2026-11-10) is to **wrap Apple's frameworks, never replace them**. For plain transcription on iOS 26, Apple's own on-device speech recognition (the Speech framework's newer analyzer API) is the first thing to wrap, and roadmap item 26 already says so. It needs no model download, no model licence and no third-party package. It has not been tried in ixOS yet.

FluidAudio adds things Apple's APIs don't offer:

- **Speaker diarization** (who spoke when): nothing equivalent in Apple's public APIs.
- **Voice activity detection and end-of-utterance** as separate building blocks.
- **Choice of model**, including languages and vocabulary boosting, and the same results on macOS.
- **Speech output** voices beyond AVSpeechSynthesizer.

Who in ixOS would use those: Relay's Chat (voice messages: transcription first, which Apple covers), Soundscape (analysis of songs is a different problem; speech models don't help), FieldKit (spoken field notes: transcription), Pocket Library (read-aloud voices). None needs diarization today. That is the deciding point.

## Scorecard

| Dimension | Rating | Reason |
|---|---|---|
| Capability | Adequate | Transcription overlaps with Apple's own; diarization and VAD are genuinely new |
| Product value | Adequate | Transcribed voice messages and field notes are useful, but the first version doesn't need FluidAudio |
| Architecture | Adequate | Fits as a native Swift adapter behind a Rust-facing trait, like the AI adapters ([forge-ai-plan.md](../../../../forge-ai-plan.md)); Rust would own the transcripts, the adapter the audio and models |
| Platform | Adequate | Shipping iOS apps use it; model memory on the free team's limits is unmeasured here |
| Development | Unknown | Not built with xtool. Source suggests an ASR-only build avoids the risky parts |
| Maintenance | Strong | Active, several contributors, many users |
| Licence | Adequate | SDK Apache-2.0; per-model licences need checking for every model offered |
| Security | Adequate | Downloads models from Hugging Face at run time; an offline mode exists. Audio stays on the device |
| Integration cost | Weak | Large package (hundreds of source files), model downloads of 200 to 600 MB for transcription, and a model-management screen per app or in ForgeKit |
| Reusability | Adequate | Several apps could use transcription, but Apple's framework serves them too |

## Verdict

**Watch.** Wrap Apple's on-device speech recognition first (roadmap item 26), in the first app that needs it. Look again at FluidAudio when an app needs something Apple doesn't provide: speaker diarization, voice activity detection as a separate step, a language Apple doesn't cover on device, or the same transcription on another platform.

The reason: FluidAudio is credible, active and more Linux-friendly than most Core ML options (compiled models, opt-out binary target), but its main capability overlaps with a free Apple framework, and adopting it would add model downloads, per-model licences and a large package for no capability ixOS currently needs.

Reopen when: an app needs diarization or VAD; Apple's speech recognition fails on the phone for a language or quality reason; or the Forge AI module wants one adapter for several speech models.

## Next steps, if wanted

1. **Apple first (recommended):** a small spike of Apple's on-device speech recognition built with xtool, transcribing one recorded voice message in Device Lab. That answers roadmap item 26 and is the baseline any FluidAudio comparison needs.
2. **When FluidAudio is evaluated for real** (not now), measure on the iPhone rather than trust the Neural Engine claim: model download size, memory at peak (under the free team's limits), cold start to first words, and sustained processing (an hour of audio: speed, heat, battery). The Neural Engine doesn't make every model practical on every supported iPhone.
3. **Keep FluidAudio ready (optional, about 15 minutes on the laptop, no phone):** an unsigned xtool iOS build of FluidAudio with `traits: []` through the build queue, to replace "Unknown" under Development with a result. Only worth it if Reuben expects to need diarization soon. No implementation PR for FluidAudio now.
