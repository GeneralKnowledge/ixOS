# Content guidelines

The curriculum in `rust/content/` is the source of truth. The app teaches what is in those files and nothing else. A future AI character may add variety to a conversation, but it never adds to the curriculum and never marks anything as correct Urdu.

## What we teach

- **Pakistani conversational Urdu**, as people actually speak it to friends and family, including the English words they mix in. Not textbook or literary Urdu, not Hindi, and not directions, colours or classroom dialogues.
- **The learner speaks the male forms.** Every phrase whose form depends on the speaker's gender is marked `speaker = "male"` and carries its female form as a `female_speaker` variant, so the learner understands women speaking. The content tests enforce this.
- **Tum and aap both.** Phrases default to tum between friends; aap forms come as `polite` variants with a usage note on when to use them.
- **Negatives with their positives.** A negative is taught next to its positive twin (`opposite`), and the distinctions that trip learners up get their own entries: nahi and mat, na, kuch nahi and koi nahi, nahi chahiye and nahi chahta.
- **Three kinds of building block.** A *word* is one word, or a pair that works as one (woh log, pasand karna, marked `kind = "word"`). An *expression* is a fixed thing said whole (mujhe nahi lagta, koi masla nahi). A *pattern* is a sentence frame with a ___ gap (Main ___ sakta hoon) and at least two example sentences, each with its Urdu script, so the learner can build new sentences. The kind follows from the entry unless it says otherwise.
- **Words in combination.** A Building Blocks word comes with its common spoken combinations (`main bhi`, not just `bhi`) and a gap line to use it in.

## Roman Urdu house style

One preferred spelling per entry, with common alternatives as `spelling` variants so typed answers in any of them are accepted. The answer checker also knows the common spellings of everyday words, below. It deliberately does not fold sounds together: a long or short vowel (kaam, work; kam, less), a doubled consonant (pata, know; patta, leaf) or q against k is a real difference, so a typed answer that differs only there is "nearly", never right.

| Sound or word | House spelling | Accepted |
|---|---|---|
| no, not | nahi | nahin, nahee |
| is | hai | he, hy |
| am | hoon | hun, hu |
| I / in | main / mein | mein, main (the checker folds the two) |
| what | kya | kia |
| good, OK | acha | achha, accha |
| fine | theek | thik |
| long vowels | aa, ee, oo | in house style only: kaam and kam are different words, so the checker never treats one as the other |

Write words as Pakistanis text them, not in a scholarly transliteration: no diacritics, no capital letters for retroflex sounds. Capitalise the first word of a phrase and proper names.

## Urdu script

Every entry has its Urdu script in `urdu`. The app doesn't show it yet; the phone's Urdu voice reads it aloud, so it has to be the same words as `roman`, punctuated the way the line should be spoken.

## Statuses

| Status | Means | Who sets it |
|---|---|---|
| `draft` | Written, not checked. Everything starts here, and the app shows it | Anyone |
| `reviewed` | Grammar, usage and naturalness checked by a competent Pakistani Urdu speaker | The reviewer, with their notes |
| `field_tested` | Confirmed natural in a real conversation of this kind | After a real conversation |

Never mark an entry reviewed on the strength of an AI's opinion, a dictionary or a Hindi source. A reviewer checks: is it natural, would a Pakistani say it this way, is the formality right, is the female form right, does the usage note match how it's used, and is the Urdu script the same words.

## Changing content

- Never rename an `id`: progress is saved against it. Add a new entry and retire the old one instead.
- Run `cd rust && cargo test`: it fails on anything the guidelines can check by machine.
