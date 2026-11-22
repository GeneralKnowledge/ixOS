# Defect hunt: bugs ordinary iPhone testing misses (November 2026)

Asked for by Reuben on 22 November 2026: look for bugs that a normal phone sitting would not catch (edge cases, interrupted operations, unexpected input, stale state, data loss, recovery), check the Rust–Swift boundary, follow whole workflows, and report only defects with evidence.

Scope: Relay, Soundscape, Checklist, Commonplace, Good Dog, Pocket Library and Urdu, Naturally, plus the starter templates for the cross-app checks. Code read at `main` 532c042. The Urdu 300-character chat limit is being fixed in the Urdu thread and is not repeated here.

**Evidence levels.** *Probe*: a throwaway Rust test ran the real code path on Linux and printed the result (the test was not kept). *Source*: read from the code, with the lines named; Swift was not compiled or run here. Nothing below was run on a phone.

## Ranked findings

| # | App | Defect | Severity | Likelihood | Evidence |
|---|---|---|---|---|---|
| 1 | Relay | A message the server or device refuses (over 4,000 characters) is wiped from the composer, so the text is lost | Data loss | Medium (pasting a long text) | Source |
| 2 | Good Dog | A session started before midnight and saved after it is refused, so the session can't be recorded | Data loss | Low to medium | Probe |
| 3 | Pocket Library | EPUB import is refused with a form message ("Keep the title under 300 characters") that the person can't act on | Import fails | Low to medium | Probe |
| 4 | Soundscape | Edit Song's errors are hidden behind its own sheet: Save seems to do nothing, and Cancel then shows the alert and drops the edits | Silent failure, lost edits | Low | Source |
| 5 | Good Dog | After a clock change, the week's remaining reminders fire an hour early or late until the app is opened | Wrong time | Certain twice a year | Source |
| 6 | Checklist | Importing the same spreadsheet twice duplicates any item whose cell has a double space or a tab | Duplicates | Low | Probe |
| 7 | Urdu, Naturally | A name over 40 characters typed in Settings is lost on leaving the screen | Lost input | Low | Source |

### 1. Relay: a refused message is wiped from the composer

**Path.** `ChatView.send()` (`examples/relay/Sources/AppHost/Messaging/ChatView.swift:484`) sets `draft = ""` (line 509) and only then starts `model.send`. chat-core refuses bodies over `MAX_BODY_CHARS = 4000` straight away (`examples/relay/messaging/chat-core/src/client.rs:60`, checked at 485). `ChatModel.send` puts the reason in `sendError`, but the composer is already empty, and the 500 ms draft autosave then saves the empty draft over the saved one. The same happens to:
- **an edit** over 4,000 characters (`client.rs:674`): `send()` puts back the stashed draft (line 503) before the edit runs, so the edited text is gone;
- **photos or documents with a long caption**: the staged files are released (line 496) as well as the text being cleared (line 491).

**Reproduce.** In any conversation (device or server), paste about 4,100 characters and tap Send. The composer empties and "Check your input: message body is longer than 4000 characters" shows; the text is nowhere.

**Impact.** The person loses what they wrote. The composer never shows the limit.

**Smallest fix.** Clear the composer only when the send succeeds, as Relay's own assistant composer already does (`AssistantViews.swift:230`) and as Urdu's chat does (`ChatScreen.swift:246`, which gives the words back). Give the edited text back if the edit fails. Export `MAX_BODY_CHARS` and show a count near the limit.

### 2. Good Dog: a session that crosses midnight can't be recorded

**Path.** Swift reads `today` when the app opens or comes back (`GoodDogModel.reload`), not at midnight. Recording ties the session to a plan activity, and `Stored::check_recordable` looks for that activity only in the plan for the local day of the moment it is saved (`examples/good-dog/rust/src/training.rs:378-389`, `today_index` at 188). After midnight there is no plan for the new day yet, so it fails with `NotFound { what: "that activity" }`.

**Probe.** Plan read at 23:50, session recorded at 00:05 with that plan's first activity: `Err(NotFound { what: "that activity" })`. The guided session ends in the same call (`record_guided_session` calls `record_session`).

**Impact.** An evening session (a late walk, a puppy's last training before bed) can't be saved; trying again fails the same way, so the only way out is to give up the session. Reminders also still think the dog hasn't practised.

**Smallest fix.** In `check_recordable`, look for the activity in any kept plan (plans are kept for `KEEP_PLANS_DAYS`), not only today's. If it isn't found anywhere, record the session as extra practice (`activity_id: None`) rather than refusing it.

### 3. Pocket Library: imports refused by the form's limits

**Path.** EPUB import and Open Library import build a `BookInput` and pass it through the add form's `validate` (`examples/pocket-library/rust/src/lib.rs:462` and `:372`), which refuses a title over 300 characters, an author over 200 and a description over 20,000 (`domain.rs:14-17`, 255-278). For an EPUB the author is every `dc:creator` joined with commas (`epub.rs:66-70`), editors, illustrators and translators included, so an anthology or edited collection easily passes 200.

**Probe.** An EPUB whose title is 305 characters: `Err(InvalidFields { fields: [FieldError { field: "title", message: "Keep the title under 300 characters." }] })`. Swift shows it as "Check the Details" (`ErrorText.swift:39`) with no form to check.

**Impact.** A valid book can't be imported at all, and the message tells the person to edit a field they can't see.

**Smallest fix.** Clip imported metadata to the limits before `validate` (cut at a character boundary, adding "…" to the title, and keep the first creators that fit, then "and others"), as Soundscape already does for file tags (`soundscape/rust/src/library.rs:1198`) and Commonplace for OPML names (`opml.rs:327`). The form keeps refusing over-long typed input.

### 4. Soundscape: Edit Song's errors are invisible

**Path.** `EditTrackSheet` (`examples/soundscape/Sources/AppHost/LibraryView.swift:1075`) calls `model.edit`, which reports a failure through `model.alert` (`ListenModel.swift:332-341`, 643). That alert is attached to the Listen stack (`ListenView.swift:91`), underneath the sheet, and SwiftUI does not present an alert from a view that is already presenting a sheet: the same problem Checklist met with Rename (friction log A39). The sheet has no alert of its own.

**Reproduce.** Edit a song and paste a title over 200 characters (`MAX_FIELD_CHARS`, `library.rs:32`), or hit a save failure. Tap Save: nothing visible happens. Tap Cancel: the alert "That is out of range." appears, and the edits are gone. The message names neither the field nor the limit.

**Smallest fix.** Give `EditTrackSheet` its own `forgeAlert`, as Checklist's rename sheet shows its own error. Have Rust name the field and the limit (as `NameTooLong { max }` already does for playlists).

### 5. Good Dog: reminders after a clock change are an hour off

**Path.** `Stored::reminders` schedules one notification a day for 7 days, each at `day * DAY_MS + at_minute - offset_ms`, with the offset taken from `now` (`examples/good-dog/rust/src/reminders.rs:53-63`). Swift passes the offset at that moment (`GoodDogModel.swift:525-528`). Every reminder after a daylight-saving change inside the week therefore uses the old offset, and the `At { unix_ms }` trigger is an absolute moment.

**Example.** On Saturday 27 March 2027 the owner opens the app with reminders at 18:00. UK clocks go forward on Sunday 28 March. Sunday's and later reminders fire at 19:00 BST, until the app is next opened (which reschedules them). In October they fire an hour early.

**Impact.** Twice a year, a calm reminder arrives at the wrong time. A phone sitting never shows this.

**Smallest fix.** Let Swift supply the UTC offset for each of the next 7 days at the reminder time (`TimeZone.current.secondsFromGMT(for:)`), one small field on `refresh_reminders`, and have Rust use the matching offset per day.

### 6. Checklist: re-importing a spreadsheet adds duplicates

**Path.** Import skips rows already on the list by comparing `same_title` (`examples/checklist/rust/src/lib.rs:343`: trim and lowercase). Stored titles go through `valid_title` (`checklist.rs:128`), which also collapses runs of spaces, tabs and newlines. A cell with "Buy  milk" is stored as "Buy milk", and the next import compares "buy  milk" against "buy milk".

**Probe.** Importing `Item,Done / Buy  milk,No` twice: both imports report `added: 1`, and the list holds `["Buy milk", "Buy milk"]`.

**Smallest fix.** Make `same_title` collapse whitespace the way `valid_title` does (a one-line change), with a test.

### 7. Urdu, Naturally: a long name is dropped on leaving Settings

**Path.** The name field has no limit (`SettingsScreens.swift:16`). Leaving the screen saves it in `onDisappear` (line 61); Rust refuses over 40 characters (`MAX_NAME_CHARS`, `lib.rs:54`), and the alert shows after the screen and its text are gone.

**Smallest fix.** Cap the field at 40 characters (exported from Rust) or keep the screen open on failure. Fits with the Urdu thread's 300-character fix, so it could ride along there.

## The same flaw in other apps

Reuben's rule is that a flaw found in one app is checked in every app and the starter. These are the classes behind the findings above.

**A Rust limit that the screen doesn't show, or loses input over.**

| App | Limit | Result |
|---|---|---|
| Relay chat | 4,000 characters | Finding 1 |
| Relay assistant | 4,000 characters | Fine: text kept on failure |
| Relay device name | 40 | Fine: rename sheet shows the error |
| Checklist item | 200 | Fine: the message names the limit, text kept |
| Good Dog dog name | 40 | Fine: Swift reads `maxNameLength` |
| Good Dog goal in own words | 200 | Minor: the field shows no limit, but Start shows the reason and keeps the text |
| Pocket Library form | 300 / 200 / 20,000 | Fine in the form (messages under each field); imports are finding 3 |
| Soundscape song fields | 200 | Finding 4 |
| Soundscape playlist names | `NameTooLong { max }` | Fine |
| Commonplace folder and feed names | 200 | Fine: OPML names are clipped; the name sheets throw and show why |
| Urdu name | 40 | Finding 7 |
| Starter (`templates/`) | none | No limit to hit |

**Clearing input before the command succeeds.** Searched every app's Swift for a field cleared before an `await`: only Relay's composer (finding 1). Checklist, Urdu chat and Relay's assistant clear after success.

**An alert from under a sheet.** Checklist's rename and Soundscape's Edit Song (finding 4) both hit this. Worth a parity rule later: a sheet that runs a command needs an alert of its own.

**A day boundary taken once.** Good Dog (findings 2 and 5) is the only app with day-based rules in Rust. Urdu recomputes the start of the day on each read; Pocket Library's 7-day window is relative.

**Duplicate checks that compare differently from how values are stored.** Checklist (finding 6). Commonplace's feed check reruns inside the insert transaction; Soundscape imports dedupe by content hash; Pocket Library EPUBs by file.

## Checked and found sound

So the next pass doesn't repeat them:
- Checklist: commands, search cancellation, stale results after a toggle (the search reruns on every change), rename errors on the sheet.
- Commonplace: two adds of the same feed (rechecked in the transaction), undo of Mark All as Read (only the latest, skips articles opened since), refresh with feeds removed meanwhile.
- Soundscape: calls, Siri and unplugged headphones (the players are watched and the buttons follow); a failed import removes the copied files; the saved queue is tidied on restore.
- Relay chat-core: retries are idempotent by message id; a queued message can't be deleted mid-send.
- Urdu chat: a refused line is given back to the field.
- Starter notes: saved on leaving the screen and the app.

## Needs one phone check (not counted as a defect)

**Pocket Library Read Aloud across a chapter change with the screen locked.** When reading aloud moves to the next chapter, `show(chapter:)` sets `pendingScroll`, and `savePosition` saves nothing while `pendingScroll` is set (`ReaderView.swift:355`). It is cleared by the reader view's `.task` after it scrolls (`crates/forge-reader/swift/ForgeReader.swift:131-142`). If SwiftUI doesn't run that task while the phone is locked, no position is saved for the rest of the listening session, and a closed app reopens at the earlier chapter. Check: lock the phone during Read Aloud, let it cross into a new chapter and a few paragraphs, force-quit, reopen. If it opens in the new chapter, drop this.

## Suggested fix PRs

Each is small and separate; Reuben picks which go ahead.
1. Relay composer keeps refused text and shows the limit (finding 1). Swift only; needs a compile check on the laptop.
2. Good Dog records sessions across midnight (finding 2). Rust only, with a test.
3. Pocket Library clips imported metadata (finding 3). Rust only, with a test.
4. Checklist dedupe (finding 6). Rust one-liner, with a test.
5. Soundscape Edit Song alert and field-named message (finding 4). Swift and Rust.
6. Good Dog per-day offsets for reminders (finding 5). Rust and Swift; FFI change.
7. Urdu name cap (finding 7), possibly in the Urdu thread.
