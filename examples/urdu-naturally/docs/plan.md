# Urdu, Naturally: the plan

Status: 2026-11-18. 0.1 (milestone M1) is built and runs inside Device Lab; nothing has been tried on a phone yet. Everything after M1 is a plan.

## The idea

Less studying. More talking. Most language apps teach you to pass their exercises. This one teaches you to hold a conversation in Pakistani Urdu: greet someone, keep the small talk going, make plans, joke around, and recover when you're lost. Progress is what you can actually do, not points.

## Product principles

1. **Conversation first.** Every word and phrase earns its place by being something people say. No directions, colours or classroom dialogues.
2. **Building blocks, then phrases.** A foundation of about 175 to 225 high-use words (question words, pronouns, negatives, connectors, time, place, describing words, verbs, reactions) lets you build your own sentences instead of only repeating memorised ones.
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

| Level | What | In 0.1 |
|---|---|---|
| Building Blocks | Foundation topics, about 175 to 225 entries in three kinds: words (recognise, recall, use in a gap line), expressions (learnt whole) and sentence patterns (build new sentences from them) | 188 in 12 topics: 137 words, 42 expressions, 9 patterns |
| 1. First contact | Hello, how are you, names, goodbye | Yes |
| 2. Keeping it going | Small talk, reactions, Conversation Glue | Yes |
| 3. Everyday life | What you're doing, what you want, what you don't | Partly |
| 4. Plans and people | Making plans, family, friends | Partly |
| 5. Banter | Teasing, jokes, exaggeration | Started |
| 6. Real conversations | Telling a story, disagreeing, recovering | Recovery phrases only |

**Everyday Negatives and Responses** is core from the start: nahi, mat, na, kuch nahi, koi nahi, nahi chahiye, nahi chahta, each with its positive twin.

**Conversation Glue** is the filler that makes you sound like a person: pata nahi, haan shayad, acha acha, matlab?, phir kya hua?, chalo, yaar.

## Milestones

### M0: Foundations (done)

The content model (words, phrases, variants, combinations, gap lines, twins, statuses), its validation, the learner model with four skills and review events, and the speech check.

### M1: 0.1, talk through five conversations (built, awaiting the phone)

Home suggests what to do next (review, the next conversation, or a topic). Learn has Building Blocks and conversation topics, search and a page per entry. Talk has five scenarios. Listen has multiple-choice rounds. Review is spaced per skill. Progress shows what you can do. Settings has your name, English support, the speech check and start over.

Phone check: open the first conversation, take a turn, relaunch, find it counted; listen to both voices in the speech check and note which Urdu voices the phone has.

### M2: Native-speaker review

Before review, an M1 quality pass (done 2026-11-18) split the Building Blocks into words, expressions and patterns, added amounts, manners and having/for/with, stopped the answer checker treating sound-alikes (kam, kaam) as the same word, made twins name each other, and made Listen name the voice actually reading.

A reviewer goes through the Building Blocks and the first 50 phrases, checking in particular the new entries' natural Pakistani usage (meherbani, zara, khush aamdeed). The app gains a reviewer mode: mark an entry reviewed or flag it, with a note, and export the notes. Finish the Building Blocks to about 200 words, then settle the starter phrases on top of them.

### M3: Sound Lab

The sounds English speakers miss: aspirated and unaspirated (k and kh, p and ph), retroflex and dental (t and the other t, d and the other d), nasal vowels, q and k, gh and g, and the throaty kh of khana against k. Minimal pairs to hear and tell apart, slow and normal speed, and recording yourself to compare by ear. Benchmark the phone's voices on these first: if they blur the pairs, Sound Lab needs recorded audio from a speaker.

### M4: Listening, four ways

1. **Hear and pick** (in 0.1): what did they mean?
2. **No subtitles**: a whole conversation by ear, then what happened.
3. **Real speed**: Omar's pace and run-together speech, then slowed down.
4. **What did they actually say?**: type or pick the words you heard.

### M5: Before You Talk, After You Talk

For real conversations with real people. **Before**: pick who you'll see, get the five phrases most likely to come up and a quick warm-up. **After**: note what you said, what you understood, what you wished you could say, and new words you heard; those become your own entries (marked as yours, not curriculum) and come back in review.

### M6: Personal language profile and phrasebook

What you say about yourself (your name, family, work, where you're from, what you like), filled in once and used in conversations. A personal phrasebook of saved and own entries. Export everything (progress, phrasebook, notes) as a file; delete everything from Settings.

### M7: Free conversation with Ali, Ayesha and Omar

Characters that hold a conversation within what you've learnt, using the curriculum as their vocabulary and never teaching from outside it. Five measures per conversation: comprehension (did you follow), response (did you answer), fluency (how quickly), recovery (did you get back on track when lost), naturalness (did you sound like a person). On-device first (Apple's model, already working in Relay); a server provider only if the phone's model can't hold Urdu.

## Speech

- **Voices**: AVSpeechSynthesizer reads the Urdu script. The speech check lists the phone's Urdu voices with a sample to judge by ear. If the voices are poor, recorded audio or a server voice comes next; no decision until we've heard them.
- **Recognition**: SFSpeechRecognizer may or may not list Urdu. If it does, it can say whether the words were right, not whether they sounded right. Pronunciation scoring stays out until something can actually do it.
- **Raw recordings** are discarded after use unless you choose to keep one.

## Privacy

- Everything stays on the phone in 0.1: no account, no server, no analytics.
- No-AI mode: when characters arrive, a setting keeps everything local and scripted.
- If a server is added later: API keys only on the server, never in the app; no logging of keys, raw audio or what you said; no public registration until the app is meant for others.

## Not now

Urdu script lessons, Hindi, grammar tables, leaderboards, and an Android version.
