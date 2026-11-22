# Urdu, Naturally

Less studying. More talking. A native iPhone app for learning conversational Pakistani Urdu: the things you actually say to friends and family, not directions, colours or textbook dialogues. It runs inside Device Lab (Apps, Urdu, Naturally).

You see Roman Urdu, hear it in the phone's Urdu voices (a woman's and a man's), and practise each phrase four ways: recognise it, recall it from the English, use it at your turn in a conversation, and understand it by ear. You judge how it went; the app never pretends to score your pronunciation. There are no points, streaks or hearts.

**Every word of Urdu in this version is a draft.** None of it has been checked by a native speaker yet; the app says so, and each entry carries its status (draft, reviewed, field-tested). See [the content guidelines](docs/content-guidelines.md).

The plan, from this first version to free conversation with Ali, Ayesha and Omar, is [docs/plan.md](docs/plan.md).

## What's in 0.1

The conversations are the course. You don't study word lists first: you have a conversation, and the words come from it.

- **Talk**: 14 conversations about what people actually talk about (how was your day, everyone at home, tea at your aunt's, have you eaten, on my way, this weekend, you look tired, the match, Eid), in three levels, with Ali, Ayesha, Omar and Khala. Their lines are read aloud in their voice; at your turn you see the English, say it out loud (or type it), then see the Urdu. Each conversation shows what you'll pick up, and ends with its words and a button to practise them.
- **Just chat**: pick Ali, Ayesha, Omar or Khala and say anything, in Roman Urdu or English. They answer with lines from the conversations, chosen by what you said, so it goes somewhere new every time. Say one of your lines and it counts as practice; say it in English and you see how to say it in Urdu (Apple's on-device model works out which line you meant, and never writes Urdu itself). Help shows what you could say.
- **Practise**: what came up in your conversations comes back, spaced per skill (missed 10 minutes, struggled 1 day, with help 2 days, on your own 4 days, confident 7 to 30 days; anything you only met, the next day). Plus listening with no text, and the lines you find hardest to say.
- **Words**: for looking things up. Search, every line by topic, and the Building Blocks the conversations are made of: 183 words, expressions and sentence patterns (*Main ___ sakta hoon*), each with combinations, gender forms and its opposite.
- **Progress**: what you can recognise, recall and use.
- **Speech check**: which Urdu voices the phone has, and whether its speech recogniser lists Urdu. Listen names the voice actually reading: a woman or a man only when the phone has that voice.

Everything runs on the phone. There is no server. The only AI is the phone's own model in free chat, picking which of your lines you meant; characters who say new things come later ([plan](docs/plan.md)).

## Layout

| Path | What |
|---|---|
| `rust/content/` | The curriculum, in TOML: `scenarios.toml` (characters and conversations, the course itself), `phrases.toml` (their lines), `words.toml` (Building Blocks). The source of truth |
| `rust/src/content.rs` | Reads and validates the curriculum: every entry in Urdu script, male forms with their female form, known topics, twins and characters |
| `rust/src/schedule.rs` | The four skills, outcomes and review intervals |
| `rust/src/chat.rs` | Free chat: who answers what across every conversation, understanding what you said (your lines, their English, or the phone's model picking a number), and choosing their answer |
| `rust/src/answer.rs` | Checks a typed answer: forgives the common spellings of the same word (nahi or nahin, hai or hy), never merges words that only sound alike (kam, less; kaam, work) |
| `rust/src/lib.rs` | The UniFFI API (`UrduCore`): home, topics, phrases, conversations, free chat, review and listening rounds, and the commands that record practice |
| `Sources/AppHost/` | The SwiftUI screens, the view model (`UrduModel.swift`) and the narrator (`Narrator.swift`, AVSpeechSynthesizer) |
| `Tests/AppHostTests/` | XCTests of the view model against the real Rust core |
| `uitests/smoke.toml` | A phone run: first conversation, first turn, still counted after a relaunch |

## Checking it

```bash
cd rust && cargo test
```

The content tests fail on any entry without Urdu script, any male form without its female form, or any reference to an unknown topic, phrase or character. On the phone, Urdu, Naturally runs inside Device Lab; every control has a `screen.element` identifier.
