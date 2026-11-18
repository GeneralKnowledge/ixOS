# Urdu, Naturally

Less studying. More talking. A native iPhone app for learning conversational Pakistani Urdu: the things you actually say to friends and family, not directions, colours or textbook dialogues. It runs inside Device Lab (Apps, Urdu, Naturally).

You see Roman Urdu, hear it in the phone's Urdu voices (a woman's and a man's), and practise each phrase four ways: recognise it, recall it from the English, use it at your turn in a conversation, and understand it by ear. You judge how it went; the app never pretends to score your pronunciation. There are no points, streaks or hearts.

**Every word of Urdu in this version is a draft.** None of it has been checked by a native speaker yet; the app says so, and each entry carries its status (draft, reviewed, field-tested). See [the content guidelines](docs/content-guidelines.md).

The plan, from this first version to free conversation with Ali, Ayesha and Omar, is [docs/plan.md](docs/plan.md).

## What's in 0.1

- **Building Blocks**: 159 foundation words in 9 topics (question words, people, negatives, connectors, time, place, describing words, verbs, reactions), each with its common spoken combinations, a gap line to use it in, its feminine and plural forms, and its positive or negative twin.
- **Phrases**: 68 conversational phrases in 6 topics, including Conversation Glue (pata nahi, haan shayad, acha acha, matlab?) and what to say when you're lost.
- **Conversations**: 5 short scenarios with Ali, Ayesha and Omar. Their lines are read aloud in their voice; at your turn you see the English, say it out loud (or type it), then see the Urdu.
- **Listen**: rounds of 8 phrases heard with no text, alternating a woman's and a man's voice.
- **Review**: spaced repetition per phrase and per skill (missed 10 minutes, struggled 1 day, with help 2 days, on your own 4 days, confident 7 to 30 days).
- **Progress**: what you can recognise, recall and use, and the phrases you find hardest to say.
- **Speech check**: which Urdu voices the phone has, and whether its speech recogniser lists Urdu.

Everything runs on the phone. There is no server and no AI in this version.

## Layout

| Path | What |
|---|---|
| `rust/content/` | The curriculum, in TOML: `words.toml` (Building Blocks), `phrases.toml`, `scenarios.toml` (characters and conversations). The source of truth |
| `rust/src/content.rs` | Reads and validates the curriculum: every entry in Urdu script, male forms with their female form, known topics, twins and characters |
| `rust/src/schedule.rs` | The four skills, outcomes and review intervals |
| `rust/src/answer.rs` | Checks a typed answer, forgiving Roman Urdu spelling (nahi or nahin, hai or he) |
| `rust/src/lib.rs` | The UniFFI API (`UrduCore`): home, topics, phrases, conversations, review and listening rounds, and the commands that record practice |
| `Sources/AppHost/` | The SwiftUI screens, the view model (`UrduModel.swift`) and the narrator (`Narrator.swift`, AVSpeechSynthesizer) |
| `Tests/AppHostTests/` | XCTests of the view model against the real Rust core |
| `uitests/smoke.toml` | A phone run: first conversation, first turn, still counted after a relaunch |

## Checking it

```bash
cd rust && cargo test
```

The content tests fail on any entry without Urdu script, any male form without its female form, or any reference to an unknown topic, phrase or character. On the phone, Urdu, Naturally runs inside Device Lab; every control has a `screen.element` identifier.
