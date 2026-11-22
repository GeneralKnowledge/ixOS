# Urdu, Naturally: the plan

Status: 2026-11-22. 0.1 (milestone M1) is merged and runs inside Device Lab. M2's first step, free chat with lines from the course, is built and waiting for the phone. Everything after that is a plan.

**Course correction (2026-11-19).** The original ask was to learn a bunch of basic Urdu to say conversational things, not to be boring like Duolingo, and to skip the textbook way. By 2026-11-18 the app had drifted into a curriculum: vocabulary lists first, flashcards, five conversations. Reuben chose a conversation-first redesign: the conversations are the course, words are picked up inside them, the word lists are for looking things up, and free conversation with the characters comes next instead of last.

## The idea

Less studying. More talking. Most language apps teach you to pass their exercises. This one teaches you to hold a conversation in Pakistani Urdu: greet someone, keep the small talk going, make plans, joke around, and recover when you're lost. Progress is what you can actually do, not points.

## Product principles

1. **Conversations are the course.** You learn by having conversations about things you'd actually talk about: your day, family, food, plans, feelings, joking around. Every word and phrase earns its place by coming up in one. No directions, colours or classroom dialogues.
2. **Words come from conversations.** Each conversation shows the words, expressions and sentence patterns it uses, and they come back in Practise. The Building Blocks (about 175 to 225 high-use entries) are what the conversations are made of, and a reference to look things up in, never a list to study first.
3. **Four skills, kept apart.** Recognising a phrase, recalling it, using it at your turn and understanding it by ear are different abilities, tracked separately (0 to 5 each).
4. **Honest about speech.** The phone can say whether it recognised words. It can't judge your pronunciation, and the app never pretends it can. You judge how it went.
5. **The curriculum is the source of truth.** AI adds variety to conversations later; it never adds to the curriculum or certifies anything as correct.
6. **Draft until checked.** Every entry is `draft` until a native Pakistani Urdu speaker reviews it ([content guidelines](content-guidelines.md)).
7. **No XP, streaks or hearts.** Nothing that punishes a missed day or rewards tapping.
8. **Personal first, public later.** Built for one learner now, with nothing that stops it serving others later.

## Locked decisions

| Decision | Choice |
|---|---|
| Variety | Pakistani conversational Urdu, with the English people mix in |
| Writing | Roman Urdu on screen; Urdu script kept for the voice only. Showing script is a later option |
| Learner's forms | Male. Female forms are heard and understood, never required |
| Formality | Tum and aap both, with when to use each |
| English | Shown for new material, fading as you learn it, always one tap away (Settings: fading, always, minimal) |
| Voices | A woman's and a man's |
| Review | Missed 10 min, struggled 1 day, with help 2 days, on your own 4 days, confident 7 days, then 14 to 30 |
| Characters | Ali (casual, the default), Ayesha (patient), Omar (fast) |
| Platform | Native iPhone app on ixOS: SwiftUI screens, a Rust core over UniFFI, inside Device Lab |
| Look | Dark charcoal, warm off-white, restrained green (ForgeUI with the green accent) |

## Three things kept apart

- **Curriculum**: what to learn (`rust/content/`). Changes only through review.
- **Learner**: what you know (per entry, per skill, with a log of review events). Saved on the phone.
- **Conversation**: a session with a character. Uses the curriculum, records into the learner, owns neither.

## Curriculum

The conversations come first: 14 in 0.1, in three levels (starting out, everyday, having fun), about greetings, your day, family, food and chai, plans, feelings and banter, with Ali, Ayesha, Omar and Khala (your aunt). The levels below are what they cover.

| Level | What | In 0.1 |
|---|---|---|
| Building Blocks | What the conversations are made of, in three kinds: words, expressions (learnt whole) and sentence patterns (build new sentences from them). Reference, not a course | 183 in 12 topics (directions-style place words dropped) |
| 1. First contact | Hello, how are you, names, goodbye | Yes |
| 2. Keeping it going | Small talk, reactions, Conversation Glue | Yes |
| 3. Everyday life | Your day, food and chai, how you feel | Yes |
| 4. Plans and people | Making plans, family, aunts, Eid | Yes |
| 5. Banter | Teasing, jokes, cricket | Yes |
| 6. Real conversations | Telling a story, disagreeing, recovering | Recovery phrases only |

**Everyday Negatives and Responses** is core from the start: nahi, mat, na, kuch nahi, koi nahi, nahi chahiye, nahi chahta, each with its positive twin.

**Conversation Glue** is the filler that makes you sound like a person: pata nahi, haan shayad, acha acha, matlab?, phir kya hua?, chalo, yaar.

## Milestones

### M0: Foundations (done)

The content model (words, phrases, variants, combinations, gap lines, twins, statuses), its validation, the learner model with four skills and review events, and the speech check.

### M1: 0.1, talk through fourteen conversations (built, awaiting the phone)

Four tabs. **Talk** is the front door: the next conversation, then all of them by level. A conversation shows what you'll pick up, plays their lines in their voice, gives you your line in English to say, and ends with the words from it and a button to practise them. **Practise** is what came up in your conversations coming back (lines and words, spaced per skill; anything you met returns the next day), listening, and the lines you find hardest to say. **Words** is for looking things up: search, every line by topic, and the Building Blocks. **Progress** shows what you can do. Settings has your name, English support, the speech check and start over.

Phone check: open the first conversation, take a turn, relaunch, find it counted; listen to both voices in the speech check and note which Urdu voices the phone has.

### M2: Free conversation with Ali, Ayesha, Omar and Khala (first step built)

What stops it being boring, so it comes next rather than last.

**Step 1, built (2026-11-22): free chat with lines from the course.** Talk has a "Just chat" section: pick someone and say anything, in Roman Urdu or English. They answer with lines from the conversations, chosen by what you said, so a chat can go from your day to food to the match in any order, and never goes the same way twice. The curriculum stays the truth: the characters never say Urdu that a model wrote, because Apple's on-device model can't write Pakistani Urdu reliably, and nothing unchecked reaches you as Urdu.

- **Said it**: what you typed is one of your lines (the same strict check as everywhere else; nearly right only counts for a line that fits there). It counts as speaking that line.
- **Meant it**: you said it in English or other words. Apple's on-device model, when the phone has Apple Intelligence, picks which of your lines you meant from a numbered list; it answers with a number, never with Urdu. Typing a line's English exactly works without the model. You see "Say it like this" with the Urdu, and that line comes back in Practise the next day.
- **Didn't catch it**: they wait. Help shows the lines that would fit.
- Characters speak lines from their own conversations, then from others in their circle (Ali, Ayesha and Omar are friends; Khala is family), never in a form that isn't theirs (a woman never says a man's form). When they run out, they have to go.
- What you type stays in memory for that chat only; it is never saved, logged or sent anywhere off the phone.

**Step 2, next: wider conversations.** The characters say something new, not only course lines, once that Urdu can be trusted: either a model that writes Pakistani Urdu well enough (checked against the native-speaker review, M3) or lines a reviewer has approved in advance. Five measures per conversation: comprehension (did you follow), response (did you answer), fluency (how quickly), recovery (did you get back on track when lost), naturalness (did you sound like a person). On-device first, behind a no-AI setting; a server provider only if the phone's model can't hold Urdu. Its Urdu is never marked reviewed.

### M3: Native-speaker review

Before review, an M1 quality pass (done 2026-11-18) split the Building Blocks into words, expressions and patterns, added amounts, manners and having/for/with, stopped the answer checker treating sound-alikes (kam, kaam) as the same word, made twins name each other, and made Listen name the voice actually reading.

A reviewer goes through the Building Blocks and the first 50 phrases, checking in particular the new entries' natural Pakistani usage (meherbani, zara, khush aamdeed). The app gains a reviewer mode: mark an entry reviewed or flag it, with a note, and export the notes. It runs alongside M2, and nothing is marked reviewed without it.

### M4: Before You Talk, After You Talk

For real conversations with real people. **Before**: pick who you'll see, get the five phrases most likely to come up and a quick warm-up. **After**: note what you said, what you understood, what you wished you could say, and new words you heard; those become your own entries (marked as yours, not curriculum) and come back in review.

### M5: Sound Lab

The sounds English speakers miss: aspirated and unaspirated (k and kh, p and ph), retroflex and dental (t and the other t, d and the other d), nasal vowels, q and k, gh and g, and the throaty kh of khana against k. Minimal pairs to hear and tell apart, slow and normal speed, and recording yourself to compare by ear. Benchmark the phone's voices on these first: if they blur the pairs, Sound Lab needs recorded audio from a speaker.

### M6: Listening, four ways

1. **Hear and pick** (in 0.1): what did they mean?
2. **No subtitles**: a whole conversation by ear, then what happened.
3. **Real speed**: Omar's pace and run-together speech, then slowed down.
4. **What did they actually say?**: type or pick the words you heard.

### M7: Personal language profile and phrasebook

What you say about yourself (your name, family, work, where you're from, what you like), filled in once and used in conversations. A personal phrasebook of saved and own entries. Export everything (progress, phrasebook, notes) as a file; delete everything from Settings.

## Speech

- **Voices**: AVSpeechSynthesizer reads the Urdu script. The speech check lists the phone's Urdu voices with a sample to judge by ear. If the voices are poor, recorded audio or a server voice comes next; no decision until we've heard them.
- **Recognition**: SFSpeechRecognizer may or may not list Urdu. If it does, it can say whether the words were right, not whether they sounded right. Pronunciation scoring stays out until something can actually do it.
- **Raw recordings** are discarded after use unless you choose to keep one.

## Privacy

- Everything stays on the phone in 0.1: no account, no server, no analytics.
- Free chat uses only the phone's own model (Apple Intelligence), and only to pick which of your lines you meant. What you type is never saved or sent off the phone. Without Apple Intelligence it still works: only your lines (and their English, typed exactly) are understood.
- No-AI mode: when characters say new things (M2 step 2), a setting keeps everything local and scripted.
- If a server is added later: API keys only on the server, never in the app; no logging of keys, raw audio or what you said; no public registration until the app is meant for others.

## Not now

Urdu script lessons, Hindi, grammar tables, leaderboards, and an Android version.
