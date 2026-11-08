# Good Dog

A native iPhone app for training your dog with short, kind, reward-based activities. Open it, see what to practise today, follow the steps, say how it went, and get a better plan tomorrow. It is the second ixOS demo ([plan](../../docs/good-dog-demo.md)), built from the [Good Dog web app](https://github.com/GeneralKnowledge/Good-Dog). Everything stays on the phone: there is no account and no server.

Good Dog gives general training guidance. It isn't veterinary care or an individual behaviour assessment.

## What it does (GD1)

| Screen | What |
|---|---|
| Welcome | The dog's name, age, the time you have most days, what would help most, and your experience |
| Today | Two or three activities chosen for today and why, progress through them, "Try something else" (twice per activity), "Only a few minutes today" |
| Exercise | Why it helps, getting ready, steps, what to look for, what to do if it's hard, safety notes, a stopwatch, and "How did it go?" with a "seemed worried or uncomfortable" switch |
| Learn | Exercises by topic for your dog's age, the training-words glossary, search across both. Training words in any text are tappable |
| My Dog | Details (editable), this week's sessions, skills in plain English, recent sessions, erase everything |

## How it is built

| Path | What |
|---|---|
| [`../good-dog-core`](../good-dog-core/README.md) | The plan engine, dog-training rules and exercise content, ported from the web app (GD0) |
| `rust/src/training.rs` | The saved data and the rules that change it: today's plan, swaps, the short plan, recording sessions, progress |
| `rust/src/lib.rs`, `model.rs` | The API Swift calls (`AppCore`) and its records |
| `Sources/AppHost/GoodDogModel.swift` | The view model (Foundation and Observation only, so it is tested on Linux) |
| `Sources/AppHost/*View.swift` | The SwiftUI screens |
| `Tests/AppHostTests/GoodDogModelTests.swift` | The model against the real Rust core |
| `requirements.toml` | What GD1 must do, and the tests that prove it |
| `uitests/journey.toml` | The journey on an iPhone, on throwaway data |

## Check and run

```bash
cargo test --manifest-path rust/Cargo.toml        # Rust, seconds
forge check swift --project .                      # the model on Linux (FORGE_SWIFT_DOCKER=swift:6.2 without Swift)
forge run ios --project .                          # build and install on an iPhone (needs the Darwin SDK and xtool)
forge ui run uitests/journey.toml                  # the journey on the phone
```

Under UI tests (`-ForgeUITestData`), the app uses a fresh folder, so it never touches your real dog, and runs on a fixed clock (Monday 9 November 2026, 09:30 UTC) so screenshots stay comparable. Adding `-ForgeUITestSampleDog` starts with a sample dog (Moss) already set up, skipping onboarding: `forge ui explore --launch-arg -ForgeUITestSampleDog`. `uitests/journey.toml` covers onboarding itself.
