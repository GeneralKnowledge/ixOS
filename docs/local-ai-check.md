# Local AI check: iPhone runbook (spike L0)

The local AI check app answers the questions that could change the plan in [local-ai.md](local-ai.md) before any Chat code is written:

1. Does Apple Foundation Models work in an app signed with a free Apple ID?
2. Does CoreML-LLM (and swift-transformers) build with xtool on Linux, download a model, compile it on the phone and run it?
3. How much memory does the default limit leave on the test iPhone, and does Qwen3.5 2B fit?
4. How quickly does generation stop when asked, and how fast does text reach Rust?
5. Does a small model stay in character for ten turns, with and without the reminder?

It is a throwaway `forge init` app. `scripts/make-ai-check-app.sh` replaces the starter screen with `scripts/ai-check/ContentView.swift`, appends `scripts/ai-check/ai_check.rs` to its Rust crate (prompt builder, characters, the `AiTokenSink` that receives generated text) and adds CoreML-LLM 1.9.0. CI compiles it for iOS with Xcode 26 (`.github/workflows/ai-check.yml`); only an iPhone run answers the questions.

## Before you start

- The phone: iOS 26 for Foundation Models (iOS 18 is enough for CoreML-LLM), and a free app slot. A free Apple ID allows three sideloaded apps; remove one first (on Cati, ServicesCheck).
- Wi-Fi on the phone, and about 5 GB free storage for all three models (the app shows free disk space).
- For Foundation Models: Settings → Apple Intelligence & Siri → turn Apple Intelligence on, and wait until its model has downloaded.
- The laptop: the ixOS checkout on this branch, xtool signed in, the iPhone plugged in and unlocked.

## Build and install (laptop, Linux)

```sh
scripts/make-ai-check-app.sh ~/forge-ai-check
forge run ios --project ~/forge-ai-check
```

The first build fetches CoreML-LLM and swift-transformers with SwiftPM. In another terminal, keep the results as they arrive:

```sh
forge device logs --grep ai-check | tee ~/ai-check-results.txt
```

Every result line in the app also goes to that log, and the app's **Share results** button sends the same lines anywhere.

## Steps (on the phone)

Each step writes PASS, FAIL or INFO lines to the Results section at the bottom of the screen.

| # | Do | Record |
|---|---|---|
| K1 | Open the app | The `start` line: device, iOS version, **available memory**, footprint, free disk |
| K2 | 2 → **Check availability** | `foundation-models: available`, or the reason it is not |
| K3 | 4 → Engine **Apple Foundation Models** → **Generate** | First text time, pieces/s |
| K4 | Same, with **Auto-stop after 20 pieces** | The answer stops at about 20 pieces; `late pieces` and `stop took` |
| K5 | 3 → **LFM2.5 350M** → **Download** (Wi-Fi) | Download time; free disk after |
| K6 | **Load**, then 4 → Engine **Downloaded model** → **Generate**; press **Stop** halfway through a second one | Load time (first load compiles), memory used, pieces/s, stop latency |
| K7 | 3 → **Unload**, **Load** again | Second load time (is the compiled model reused?) |
| K8 | 3 → **Qwen3.5 0.8B** → Download, Load, Generate, Stop halfway | As K5–K6. The first load compiles four chunks and can take minutes; it also fetches the tokenizer from Hugging Face |
| K9 | 5 → **Mia**, reminder on, ten messages; then **Leo** the same; with Qwen3.5 0.8B | For each reply: in character or not (write it down); the per-turn lines |
| K10 | K9 again with the reminder off, five messages | Does it drift sooner? |
| K11 | K9 with Engine **Apple Foundation Models**, five messages | In character? Any refusals? |
| K12 | Airplane mode test: force-quit the app, Control Centre → airplane mode on (Wi-Fi off too), open the app, load Qwen3.5 0.8B, generate | Works offline, or the error (the tokenizer may need its cached copy) |
| K13 | Wi-Fi back on. 3 → **Qwen3.5 2B** → Download, Load, Generate | Only after K1's available memory is known. A crash on load means the default memory limit is too low: `forge device crashes` on the laptop shows a jetsam report |
| K14 | 6 → **Push 10,000 pieces into Rust** | µs per push |
| K15 | 1 → **Record memory…** after a few minutes of generation | Thermal state and battery |

Then **Share results** (or keep `~/ai-check-results.txt`) and paste the lines into the thread.

## What the results decide

| Result | Decision |
|---|---|
| K2/K3 pass | Foundation Models is the L1 baseline. If it fails only because Apple Intelligence is off, L1 shows that reason; if free-team apps are refused, L1 uses CoreML-LLM only |
| The build fails on Linux | Try the llama.cpp XCFramework adapter instead (local-ai.md, risks) |
| K6/K8 load and memory | The model library's memory check (needed MB against `os_proc_available_memory`) |
| K13 | Whether Qwen3.5 2B is offered at all |
| K6/K8 stop latency, late pieces | Whether the Qwen3.5 adapter needs a patched generator for prompt stopping |
| K9–K11 | Whether the reminder stays in the harness, and how often |
| K12 | Whether the tokenizer must be downloaded with the model and loaded from disk |

Results go in the L0 table in [local-ai.md](local-ai.md#l0-results) and the On-device AI rows of the [capability ledger](capability-ledger.md).
