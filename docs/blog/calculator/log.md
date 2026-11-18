# Calculator expedition: field notes

What actually happened while exploring and recreating Calculator, for the blog post ([draft](draft.md)) and [the plan](../../research/app-as-specification.md). Times are UK time. Claims about the phone are only what a person saw on it.

Every finding carries a tag:

- **Observed:** captured directly; says where (a tree file, a screenshot, a recorded result, a file in the repository).
- **Inferred:** believed from the evidence; names the evidence.
- **Unknown:** not worked out yet.

Each entry also names the activity it belongs to: survey, collect, map, reconstruct or verify.

## 2026-11-17: before the expedition

**20:10.** Reuben is sent a blog idea: a tool that reads an app's accessibility tree, walks its screens breadth first, records state hashes and screenshots, replays sequences, and compares an original with its recreation. His reaction: "I don't think we are quite there."

**20:13.** He asks for a plan: find a relatively simple app, map it through the accessibility tree, try to rebuild it. **20:16:** the plan goes up as [PR #318](https://github.com/GeneralKnowledge/ixOS/pull/318).

Checking the equipment before setting off:

- **Observed** (repository, forge-uitest): reading any app's tree and driving it safely already works, `forge ui tree --app` and `forge ui run --any-app` behind a policy file the person writes. First done on the Claude app on 2026-11-08.
- **Observed** (forge-lab `parked/`): the explorer that walked screens, hashed them by structure and saved screenshots and a transition graph was built and run on the phone, then parked on 2026-11-15. It has no command today.
- **Observed** (`examples/commonplace/docs/benchmark/spec.md`): an earlier expedition, NetNewsWire against our reader Commonplace, was planned on 2026-11-10 and half built, but the original app was never walked. Every row of its spec is still "Expected".
- **Observed:** nothing has ever compared two apps.
- **Observed:** one of our own docs said the parked explorer "still works". It doesn't. Corrected in #318.

Choosing the specimen:

- **Observed:** Calculator ships with iOS, so it needs none of the free team's three sideload slots, which are all in use.
- **Inferred** (it's a calculator): the same keys always give the same display, which is what makes it comparable. To be confirmed in R1 by recording the same sequences twice.
- **Unknown:** whether WebDriverAgent can read an Apple app's tree at all. No Apple app has been tried. This is what R0 answers.
- **Unknown:** whether every key has a label, and whether the display's value is readable from the tree.

The plan's key idea: compare values before screens. A table of key sequences and what each leaves on the display, recorded from the original and replayed on the copy, needs no explorer at all and can also run as a plain Rust test on Linux.

**20:19.** Reuben decides the explorer question waits until R3, once the value comparison has results.

**20:29.** Reuben shares his draft of the post, "silly" and fictional by his own description. Three points didn't match the plan and were corrected: the tree is read through WebDriverAgent over a USB cable (xtool builds our copy, it doesn't inspect the original); the copy reproduces behaviour, not Apple's look; and screenshots aren't compared, because the two apps are meant to look different.

**20:37.** Reuben reframes the whole thing as an expedition: what can you learn about software by exploring everything it lets you observe? Five activities (survey, collect, map, reconstruct, verify), five representations (visual, structural, behavioural, navigational, temporal), and every finding tagged Observed, Inferred or Unknown. These notes, the draft and the plan now follow that.

**20:40.** Reuben reviews #318 and keeps the direction, with three tightenings, all now in the plan and the draft:

- Every expedition produces the same six outputs (specimen profile, evidence catalogue, field map, findings register, reconstruction record, verification report), so the next one starts from a template. They'll live in `docs/research/expeditions/<specimen>/`.
- The accessibility tree is one map, not the boundary: screenshots, unusual interactions, persistence across a relaunch, effects on the rest of the phone, and errors and unreachable controls all count. R0 now includes a quick relaunch check.
- A failed investigation is a finding. If Calculator's tree can't be read, that goes in its findings as an Observed limitation, and those findings are kept even if the expedition moves to Voice Memos.

He also asked for no new infrastructure before R0: get the phone connected and see what the calculator gives up.

## 2026-11-18: R0, survey

**00:43.** Reuben asks to do the Calculator test now. Phone plugged into the laptop; the laptop runs the commands, Reuben does anything on the phone.

- **Observed** (`forge device list`, 0.14 s): Cati, iPhone 15 Pro Max, iOS 26.3.1, on USB.
- **Observed** (`forge device apps --all`): Calculator is not listed; system apps aren't. **Inferred:** its bundle id is `com.apple.calculator`. Confirmed only if the tree read below works.
- **Observed:** NetNewsWire is installed (`com.ranchero.NetNewsWire.iOS`).
- **Observed, first finding about our own equipment:** the expedition stopped at the gate. Forge refuses to read an app it didn't build unless the person passes `--any-app` and their own `~/.config/forge/device-control.toml` lists the app, and the laptop session would not edit that file or pass the flag without Reuben's own words. Exactly what the safety design intends; it also means every new specimen starts with a person saying yes.

**00:47.** Reuben: "yes, edit the policy and use --any-app". The policy file is backed up and now lists Calculator at `observe`: look, never tap.

**00:48. The first look at the specimen** (portrait main screen, folder `~/expeditions/calculator/r0-20261118T0044Z` on the laptop):

- **Observed** (failure as finding): `forge ui tree --app com.apple.calculator` fails in 6.2 s with "com.apple.calculator is not installed on the phone". Forge's `--app` only knows apps in the installed list, and system apps aren't in it, so Forge can't bring Calculator to the front. A person has to open it. Reading whatever is in front with `--any-app` alone works.
- **Observed** (`forge --json ui tree --any-app`, 1.8 s; screenshot 1.1 s): 44 elements, 25 labelled, 25 with an identifier. Forge's accessibility audit found nothing to complain about.
- **Observed:** all 20 keys are there, as type **Key** rather than Button, each with a plain label (7, Divide, Change Sign, Point, All Clear…) and an English-word identifier (Seven, Divide, Negate, Decimal, AllClear). **Inferred:** the identifier set is enough to drive every key without coordinates.
- **Observed:** All Clear is the last key in the tree but the second key on screen (top row: delete, AC, %, ÷). Tree order is not screen order.
- **Observed:** the display is a StaticText inside a scroll view labelled "Edit field". Its label and value are "0" preceded by an invisible left-to-right mark (U+200E). The number is readable, but anything comparing it must strip that mark first.
- **Observed:** the navigation bar has History (`SidebarButton`) and Change Mode, whose identifier carries the current state: `Mode: basic; unitConversion: false`. **Inferred:** the mode can be read straight from the tree.
- **Observed** (screenshot): a ">" tab on the left edge with nothing matching it in the tree. **Unknown:** what it is. **Inferred:** probably the history sidebar's handle.
- **Observed** (NetNewsWire, for comparison, showing an article): 116 elements, 32 labelled, only 4 identifiers; the audit printed 16 warnings (12 missing identifiers, 3 small tap targets, 1 duplicate identifier).

**R0 exit met:** every key has a label and the display's value can be read from the tree. Calculator stays the specimen; Voice Memos isn't needed.

**00:52. Round 2, by hand.** Reuben turned the phone sideways, then typed 12 + 7 =, closed Calculator from the app switcher and reopened it, and sent his own screenshots ([landscape](images/r0-landscape.png), [after reopening](images/r0-after-relaunch.png)).

- **Observed** (landscape screenshot): sideways is **not** the scientific calculator. It is the same basic keypad stretched into pill-shaped keys, in a different arrangement: 7 8 9 delete ÷ / 4 5 6 C × / 1 2 3 % − / ± 0 . = +. The plan assumed "a scientific layout in landscape"; that assumption was wrong. **Inferred:** scientific mode is reached through the Change Mode button instead, whose identifier says `Mode: basic`.
- **Observed** (landscape screenshot): the clear key reads **C**, not AC, while the display shows 0. **Unknown:** why. On the portrait screen in round 1 it was All Clear.
- **Observed** (after-relaunch screenshot): after closing Calculator from the app switcher and opening it again, the display still shows **19**, with the expression **12+7** above it in grey. The last calculation survives being closed. **Inferred:** it is saved, not just kept in memory, since closing from the app switcher ends the process.
- **Observed:** the expression line ("12+7") is new compared with round 1's single display. **Unknown:** whether it is a second element in the tree, and what its label is. The laptop reads the tree next.

- **Observed** (failure as finding): when the laptop went to read the reopened screen, the phone had dropped off USB (`cannot reach usbmuxd`, and `lsusb` showed no Apple device). No trees for landscape or the reopened screen yet; Reuben's screenshots are the only evidence for those two steps. The phone falling off the cable is a known nuisance on this laptop, not something Calculator did.

**00:57. Landscape, read by the laptop** (phone back on USB, 12 + 7 = 19 showing, sideways; `forge --json ui tree --any-app` 8.1 s, then 2.1 s for a second read to rule out timing):

- **Observed:** the same 20 keys with the same ids and labels; no scientific keys. In landscape the tree order matches the screen, row by row (in portrait it didn't).
- **Observed, the big one:** **the answer is not in the tree.** Neither "19" nor "12+7" appears anywhere: 48 elements, 0 with a value. A new element, "Last Expression" (`StandardResultView`), and the "Edit field" (`StandardInputView`) are both there, but each holds only invisible, empty containers. In round 1, portrait showing 0, the number was a readable text element. Two reads gave the same result.
- **Observed:** the clear key showed **AC** on screen this time (it showed C in Reuben's earlier landscape screenshot with 0 on the display), but its label and id stay "All Clear" / `AllClear` either way. **The tree doesn't reveal the AC/C state;** only the screenshot does.
- **Unknown:** whether the missing answer is a landscape thing, a "there is a result" thing, or a relaunch thing. Next read: portrait, same 19 showing.

**00:59. Portrait again, same 19 showing** (`forge --json ui tree --any-app`, 7.4 s):

- **Observed:** the text is back. The result is a text element "19" and the last expression a separate text element "12+7", each inside its own scroll view (`StandardInputView` "Edit field" and `StandardResultView` "Last Expression"); neither text has an identifier. **So the missing answer is a landscape thing,** not a relaunch thing. Answers have to be read in portrait.
- **Observed:** the invisible left-to-right mark (U+200E) appears before *every part* of the expression, not just once: `‎12‎+‎7`. A comparison has to strip all of them, not just a leading one.
- **Observed:** the 12 + 7 survives closing and reopening the app, and the tree shows it (not just the screenshot).
- **Observed:** All Clear has moved in the tree from last place (round 1, showing 0) to second, right after Delete, matching where it sits on screen. **Inferred:** the tree order depends on the key's state (AC versus C). **Unknown:** what the key read in round 1; we didn't keep a note of it.
- **Observed:** the "Last Expression" area only appears in the tree once there is a last expression.

**01:00. History open** (portrait; `forge --json ui tree --any-app`, 8.5 s):

- **Observed:** History opens as a half-height sheet. In the tree it is an *Alert* (`HistoryView`) with Edit (no id) and Close (`CloseButton`); the keypad stays in the tree, marked hidden. Sections "Today" and "Previous 30 Days".
- **Observed:** each entry is one element (`HistoryRecordCell`) whose spoken label is the whole sum, "Input, 12 + 7, result, 19", with two texts inside, `InputRecord` and `ResultRecord`, again full of U+200E marks. 12 + 7 is there under Today: History survives the relaunch too.
- **Observed:** results in History are rounded (to three decimal places in one older entry, with "…" added to the spoken label when cut short) and use a thousands separator in another. **Unknown:** whether the main display rounds the same way. That's an R1 sequence.
- **Observed:** every entry reuses the same three ids, so Forge's audit flagged 9 duplicate identifiers, plus Sheet Grabber and Edit without ids and Sheet Grabber as a small tap target (74 × 26 pt). The first audit warnings Calculator produced at all.
- **Observed:** a scroll bar reading "9 pages": History holds far more than the four entries on screen.
- **Observed** (failure as finding): the ">" tab on the left edge never appeared in the tree, in any of the four readings. **Unknown:** what it is.

**R0 done at 01:00**, about 17 minutes from the first command, including one dropped USB connection. Reuben's earlier calculations in History aren't recorded here beyond what they show about formatting.

**What surprised us, in one list:** sideways is not scientific; the answer vanishes from the tree sideways; invisible marks inside every number; tree order changes with the clear key's state; History is an "alert"; Forge can't open a system app itself.

**01:06.** Reuben, after R0: turning the phone by hand is understandable, but the system should press buttons like History itself rather than ask him. R1 is planned that way: Forge presses keys, History and even the rotation (`forge ui orientation` exists), which needs Calculator moved from `observe` to `operate` in his policy file.

**07:31.** Reuben: "yes, move Calculator to operate". The laptop's policy file now says `"com.apple.calculator" = "operate"` (previous version kept as `device-control.toml.bak-observe`).

- **Inferred** (from Forge's commit gate, before any run): the Delete key can't be pressed unattended. Its label is "Delete", which is on Forge's list of commit words, so at `operate` Forge asks at the terminal before tapping it and refuses when nobody is there. Calculator's backspace looks, to a safety rule, exactly like deleting your files. The sequence table leaves it out.

R1's table is written: 34 sequences ([sequences.toml](../../research/expeditions/calculator/evidence/sequences.toml)), each with the question it asks and no expected answer, and a script that turns them into one `forge ui run` file (about 300 steps), runs it, and reads each answer from the tree saved beside each screenshot ([run_sequences.py](../../research/expeditions/calculator/run_sequences.py)). Forge also opens and closes History and turns the phone itself at the end.

**09:46.** Reuben: ignore landscape for now, and run the test autonomously, Delete included. The only way Forge allows that without a person at the terminal is the `full` level for Calculator alone (no commit gate for that one app; secure fields and the hard refusals still apply). The table gains five Delete sequences (39 in all) and loses the rotation.

## Next

R1, collect and map: the sequence table (ordinary sums, repeated equals, percent, sign change, huge numbers, rounding, a relaunch mid-sequence), recorded twice in portrait. Taps need Calculator at `operate` level rather than `observe`, which is Reuben's call.
