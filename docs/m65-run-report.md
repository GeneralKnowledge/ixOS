# M6.5 stage A: guided exercise session (GD-7), no phone

Date: 2026-11-08. Branch `m65-good-dog`, from `e67c8af`. Nothing was installed and no device tool was called.

## What changed

**Rust (state and rules)**

- `rust/src/guided.rs` (new): `GuidedSession`, a UniFFI object that lives only in memory. Its cards are "get ready" (the preparation, with the safety note), then each step, then the outcome card (what to look for). `card()`, `next()` and `back()` return a `GuidedCard`. Each card has its kind, its position, the step number and total, its text, and `can_go_back`, `can_go_next`, `can_record` and `recorded`. Moving past either end is refused with `InvalidInput`. A session can be recorded only on the outcome card, and only once. While a record is being saved, the session can't move. A failed save, or a cancelled Swift task, returns the session to the outcome card (`RecordingClaim`) so the owner can try again.
- `AppCore::start_guided_session(exercise_id, activity_id, now)` starts on "get ready" and saves nothing. It refuses a session that couldn't be recorded at the end: no dog, an unknown exercise, an activity that isn't today's, or one already recorded.
- `AppCore::record_guided_session(session, result, welfare_concern, now)` claims the session and then calls the existing `record_session`. The rules, the history entry, the skill progress and the feedback message are therefore the same ones the exercise screen produces.
- `training.rs`: the checks at the top of `Stored::record` became `Stored::check_recordable`, which both routes now use. The behaviour is the same as before.
- Logging goes through `forge_runtime::logging` (step count and whether the session is planned). Nothing the owner typed is logged.

**Swift**

- `GuidedSessionModel.swift` (new model file; Foundation, Observation and ForgeFFI only): it keeps the latest card from Rust and exposes `next()`, `back()` and `record(...)`. `GuidedCard.heading` gives "Get ready", "Step 2 of 4" or "How did it go?".
- `GoodDogModel`: `startGuidedSession(exerciseId:activityId:)` and `recordGuided(...)`. After recording it does what `record` does: it increments `sessionCount` (the haptic) and reloads.
- `GuidedSessionView.swift` (new): a full-screen cover with these parts:
  - The session timer is in a bar at the top of every card.
  - The heading uses `.largeTitle` and the text uses `.title2`. Each card scrolls, so long steps fit at the largest text sizes.
  - "If It's Difficult" is on the "get ready" card and every step card. It opens a sheet over the card, so Done returns to the same place.
  - Back and Next (Save on the last card) are at the bottom, at least 56 pt tall and full width. They sit side by side, or stacked (Next on top) at accessibility text sizes.
  - VoiceOver focus moves to the heading on each new card. Glossary links work inside the cover, because it has its own `termSheet`.
  - Only system colours and materials are used, for dark mode.
- `ExerciseView.swift`:
  - A "Start Guided Session" button in the first section opens the session. It is hidden once the activity is done. Each tap starts a new session from "get ready".
  - The timer is now owned by the exercise screen and shared with the session. The session starts the timer if it isn't running and stops it on save.
  - The outcome choice is now one view, `OutcomeChoice`, used by both "How Did It Go?" and the session's last card. Both offer exactly the same choice, with the same `record.result.*` and `record.welfare` identifiers.

**New identifiers**:
- `exercise.guided`
- `guided.progress`, `guided.text`, `guided.timer`, `guided.timer.start`, `guided.timer.stop`
- `guided.back`, `guided.next`, `guided.save`, `guided.close`
- `guided.ifDifficult`, `guided.ifDifficult.text`, `guided.ifDifficult.done`

The alert after saving reuses `feedback.ok`. Each control has a spoken label, for example "Next: step 3", "Back to get ready", "Close guided session" (with the hint "Nothing is recorded.") and "Save session".

**Requirement, tests and docs**

- GD-7 is in `requirements.toml`. Its twin evidence is `GoodDogModelTests.testAGuidedSessionStepsThroughAndRecordsLikeTheExerciseScreen` and `testLeavingAGuidedSessionPartWayRecordsNothing`. Its device check is `uitests/guided.toml`.
- New Rust tests: three in `guided.rs` for the state rules, and six in `tests.rs`. One of them records the same session both ways in two stores and checks that the history, progress, Today and the saved JSON file are identical. Another pins what the UI test depends on (see below).
- `uitests/guided.toml` (36 steps):
  1. Launches with `-ForgeUITestData -ForgeUITestSampleDog` and opens today's first activity.
  2. Starts the guided session and checks "Get ready", the timer and that Back is disabled.
  3. Steps to "Step 2 of 4", opens "If it's difficult", closes it, and checks it is still on step 2.
  4. Steps to the outcome card, checks that Save is disabled, goes back to "Step 4 of 4" and forward again.
  5. Records "getting there", taps OK, checks `exercise.done`, goes back, and expects `today.activity.0` to have the value "Done".
- `crates/forge-uitest/src/script.rs`: a test that `journey.toml` and `guided.toml` parse, so a mistake in a step shows up on Linux rather than on the phone.
- `Generated/` was regenerated by `forge_check` and `forge_build_ios`; it was not edited by hand. The app's `AGENTS.md` has a project note, and `docs/good-dog-demo.md` says GD-7 is built but not yet on the phone. The capability ledger is unchanged, because no framework capability changed.

## What the checks showed

| Check | Result |
|---|---|
| `cargo test` (app crate) | 23 passed |
| `cargo clippy --all-targets -D warnings` (app crate, forge-uitest) | clean |
| `cargo fmt --all --check` | clean |
| `forge_check` (Rust, then the Swift model on Linux) | ok, 9 Swift tests passed; GD-1 to GD-7 twin passed |
| `forge_build_ios` (Darwin SDK, compiles the SwiftUI) | ok on the first try, no Swift warnings (only the usual lld "platform iOS" link warnings) |
| `forge_verify --twin-only` | ok: 7 of 7 proven on the twin; no device or person verdicts yet |
| forge-uitest example-file parse tests | 3 passed |

Not run: `scripts/check-bindings.sh` and the `forge bindings generate` CLI. Both needed approval that wasn't given in this session. `forge_check` and `forge_build_ios` regenerate the bindings from the current Rust before they compile, so they built against bindings that match. Still, run `make validate` before merging.

What I found and fixed along the way:

- The first version of the pinning test used a placeholder value so that it would print the real one. It showed that the sample dog's first activity is `ex-name-response` with 4 steps, and the UI test is written for that.
- My first parse test expected the wrong number of steps for `guided.toml` (39 instead of 36). I corrected the count.
- `-ForgeUITestSampleDog` on its own does not skip onboarding. `GoodDogModel.launchSampleDog` only honours it together with `-ForgeUITestData`, which also keeps the test away from the owner's real data. The UI test therefore passes both arguments. This differs from the brief's wording ("launches with -ForgeUITestSampleDog"), but it is what the app requires.
- Inside the full-screen cover, glossary links would have tried to present the term sheet from the tabs underneath, which can't present while the cover is up. The cover and the "If it's difficult" sheet now each attach their own `termSheet`.

## What needs checking on the phone

1. **Run `uitests/guided.toml`** (`forge ui run examples/good-dog/uitests/guided.toml`). This needs the phone and proves GD-7's device check. The steps most likely to need adjusting:
   - `expect = { id = "today.activity.0", value = "Done" }`: the "Done" value is set on the row inside the `NavigationLink`, and the identifier is on the link. I haven't confirmed that WebDriverAgent reports that value on the element with the identifier.
   - `expect = { id = "guided.timer" }`: the identifier is on a `Text` inside a `TimelineView`.
   - `has_label` on `guided.progress` assumes the heading's label is exactly its text.
2. **Re-run `uitests/journey.toml`.** The exercise screen has changed: it has a new "Start Guided Session" button, and the timer buttons are labelled "Start timer" and "Stop timer". The `3-exercise` (and maybe `4-recorded`) baselines will therefore show as changed. These are intended changes, but a person should look at the diff before `forge_accept_screenshots`. The new `guided.toml` screenshots have no baselines yet.
3. **Look at the session on the phone**, in light and dark mode (`appearance: dark`) and at the largest text size:
   - The cards, the top timer bar, the stacked Back and Next buttons at accessibility sizes, and the "If it's difficult" sheet.
   - The outcome card at the largest text size: the `.largeTitle` heading inside a Form row.
   - That the buttons are easy to hit with one hand.
   - That VoiceOver moves to the heading on each new card.
4. **Leaving part way, by hand:** close the session on a step and check that nothing appears in My Dog. Start it again and check it opens on "Get ready". Then terminate the app during a session, reopen it, and check the same. The twin tests prove this against the real Rust core; on the phone it needs a person or a `terminate` step on real data. `-ForgeUITestData` starts fresh on every launch, so a UI test can't check it across a relaunch.
5. **Haptics:** a selection tap on each card change and a success haptic on save. These can only be felt on the phone.

## Stage B: on the phone (Cati, iPhone16,2, iOS 26.3.1, free team), about 9 minutes

All runs used throwaway test data with the sample dog. The owner's own data was never opened.

**How the app was launched.** `forge_launch_app` takes an appearance and a text size but no other launch arguments. Launching through it would have opened the owner's real data, so I didn't use it to launch. Instead, each launch was the first step of a test file with `args = ["-ForgeUITestData", "-ForgeUITestSampleDog", ...]`:
- `.forge/look/guided-dark.toml` is a throwaway file, not a requirement test. It adds `-ForgeAppearance dark`, takes a screenshot and a tree of the exercise screen and each kind of card, and ends on a step card for the audit.
- `uitests/guided.toml` is the GD-7 test.

**Order of work.** The Stage B brief said not to run the UI test yet, but its final line asked for the guided UI test among the most important checks, so I ran it once, after the fix.

| Step | Tool result |
|---|---|
| Install (replaces the Good Dog build already on Cati; no new app slot needed) | `forge_install_device` ok, verified |
| Dark-mode look | `forge_run_ui_test` on `guided-dark.toml`: 25 of 25 steps passed. 5 screenshots, all "new" (no baselines), plus trees |
| Audit of the step card (dark) | `forge_ui_audit`: 0 errors, 1 warning (below) |
| Log | the run's `device.log`: Rust logged `guided session started: 4 steps, planned true`. No errors or faults from the app; the one `<Error>` was AXRuntime "Unknown client", which comes from the UI test runner |
| Fix, rebuild, reinstall | `forge_install_device` ok. It reported `verified: false`, so the next run served as the check that the new build was on the phone: it showed the new button |
| GD-7 UI test | `forge_run_ui_test` on `uitests/guided.toml`: **36 of 36 passed** in 33 s. Every step passed, including `today.activity.0` value "Done", `guided.timer` and the `has_label` checks. 3 screenshots are "new" (no baselines yet; not accepted) |
| Crashes | `forge_read_crashes`: none |

What the screenshots showed in dark mode, all readable with system colours:
- **Exercise screen:** "Start Guided Session" in the first section.
- **"Get ready" card:** the timer bar, the "Get ready" heading and text, "If It's Difficult", and Back (disabled) and Next.
- **Step card:** "Step 3 of 4" with a progress bar.
- **"If it's difficult" sheet:** returned to the same step.
- **Outcome card:** "How did it go?", what to look for, the three results, the welfare toggle, and Save (disabled until a result is chosen).
- **Timer:** it kept running across the cards (0:01 on "Get ready", 0:11 on the outcome card, 0:27 back on step 3).

### Problems found

1. **The timer's Start and Stop buttons were too small to hit while holding a lead.**
   - Shown by: `forge_ui_audit` on the step card: `small_tap_target`, `guided.timer.stop` "Stop timer", 59 x 20 pt, under 44 x 44.
   - Fix: `SessionTimer` (in `ExerciseView.swift`) now uses bordered buttons with a frame of at least 44 x 44 pt. This applies on the exercise screen and in the guided session.
   - Checked by: the GD-7 run's `2-step` screenshot shows Stop as a bordered button about 44 pt tall. I did not re-run the audit on the new build before the time ran out.

### Not done, or still open

- **Light mode was not looked at on the phone.** The GD-7 run, launched without `-ForgeAppearance`, also came out dark, so Cati's own setting appears to be dark. A light-mode look needs `-ForgeAppearance light` in a look file.
- **Largest text size**, as the owner asked, was not checked on the phone. It is covered only by the Linux build and the code:
  - Back and Next stack at accessibility sizes.
  - The timer row falls back to stacked through `ViewThatFits`.
  - The cards scroll.
  - The tree showed the heading and text as separate elements with their own identifiers, which nothing truncates.
- **The audit was not repeated** on the rebuilt app, nor run on the get-ready and outcome cards. The outcome card is a Form, where the rows count as the tap targets.
- **A design point, not changed:** the exercise screen now has two full-width filled buttons, "Start Guided Session" and "How Did It Go?". In the screenshot the guided button also shows no icon. This is worth a person's look; the evidence doesn't show it is wrong.
- **Screenshots not accepted:** the `journey.toml` baselines (the exercise screen changed) and the new `guided` screenshots still need a person to look at them before `forge_accept_screenshots`.
- **The fix after Stage A** (`ExerciseView.swift`) was compiled by `forge_install_device`'s build only; `forge_check` was not re-run. It doesn't touch the model files.

## Stage C: UI tests on the phone (Cati), stopped after about 5 minutes when the phone disappeared

| Run | Result |
|---|---|
| `uitests/guided.toml` (GD-7) | **36 of 36 steps passed** (run `2026-11-08T18-32-37-ui-run`). Its 3 screenshots are "new" (no baselines). None were accepted |
| `uitests/journey.toml` (GD1), first run | **All 29 steps passed** (run `2026-11-08T18-33-11-ui-run`). The run still failed, because all 6 screenshots were "changed" against their baselines |
| `journey.toml`, second run (light mode pinned, since reverted) | Failed at step 5, `tap onboarding.start`: "WebDriverAgent closed the connection without replying". The watcher then reported "no connected device has UDID 00008130-00164D583646001C" |
| `forge_device_status` | `stage: device`, "cannot reach usbmuxd ... Plug the iPhone in and unlock it". **The phone disappeared, so phone work stopped here** |
| `forge_ui_explore` with `-ForgeUITestSampleDog` | **Not run**: the phone was gone |

### The journey's screenshot failure, and a wrong fix I undid

- **What I assumed first.** Every screen differed, including onboarding, which this change doesn't touch, and Cati was in dark mode. So I took the baselines to be light and the phone's setting to be the cause. I pinned `-ForgeAppearance light` in both test files and renamed the journey baselines with the `-light` suffix.
- **Why that was wrong.** The second run did launch in light mode: its onboarding screenshot is light, and it differed from the baseline by 99%. I then opened the baseline itself: `1-onboarding@1290x2796.png` is **dark**. The GD1 baselines were recorded with the phone in dark mode, so appearance was not why the first run failed.
- **Undone.** I renamed the baselines back and removed the pin from both test files. They are exactly as they were when `guided.toml` passed 36 of 36. No baseline was accepted or changed.
- **What the first run's diffs actually show.** On the screens this change doesn't touch (onboarding, Today, My Dog, the term sheet), the changed regions are small patches of a few hundred pixels (about 13 x 13 pt) on text. Examples: "Also called, R+", "RELATED WORDS", the term's explanation. iOS's floating side tab on the left edge also sits in a different place (near the top in the baseline, lower down now); it belongs to the system, not to Good Dog. I didn't get to look at the diff images (`visual/*.diff.png` in that run) before the phone went. So I can't yet say whether these patches are rendering noise to tolerate, a real change in the app, or the side tab. 3-exercise and 4-recorded are expected to differ, because of the new "Start Guided Session" button and the bigger timer buttons. Those need a person's look before `forge_accept_screenshots`.

### Final result

- **GD-7:** the guided-session UI test passed on the phone: 36 of 36, twice (Stage B after the fix, and Stage C).
- **GD1 journey:** every step passed, but the visual check fails until a person has looked at the 6 diffs. I did not accept them, because I couldn't tell why the unchanged screens differ.
- **Not done:**
  - The exploratory walk (`forge_ui_explore`).
  - Rerunning the journey after a decision on its baselines.
  - Re-auditing the rebuilt timer buttons.
- **Next, once the phone is back:**
  1. Open `.forge/runs/2026-11-08T18-33-11-ui-run/visual/*.diff.png`, accept only what this change meant, and rerun `journey.toml`.
  2. Run `forge_ui_explore` with `-ForgeUITestData -ForgeUITestSampleDog`. The sample dog only takes effect together with `-ForgeUITestData`.

## Record (stage D, no phone)

`forge_check` was run once more: ok, 9 Swift tests passed, and GD-1 to GD-7 twin passed (`.forge/runs/2026-11-08T18-36-46-check-swift`). Then I ran `forge_verify run: true`.

**Where it is.**
- `examples/good-dog/.forge/runs/2026-11-08T18-36-48-run/evidence.json`
- `examples/good-dog/.forge/runs/2026-11-08T18-36-48-run/report.md`

**What it says.**
- Verdict **incomplete**: 3 of 7 requirements proven (GD-2, GD-4 and GD-5, which have no device check). GD-1, GD-3, GD-6 and GD-7 are open.
- 0 builds, 0 UI test runs on the phone, no tool call failed, no crashes.
- Device checks were "not run: the run had no phone (forge mcp --no-device)".
- None of the 7 requirements has been accepted by a person. No acceptance was recorded.

**Checked against what happened: the record covers Stage D only.** It counts from `since 2026-11-08T18-36-31`, which is when the current Forge MCP server started, and its journal holds 1 tool call. The server was restarted between stages: there are journals at 18:06:43 (stage A), 18:20:17 and 18:20:50 (stage B), 18:29:12 (stage C) and 18:36:31 (stage D). So its "0 builds", "0 UI runs", "no tool call failed" and "no phone" are true of Stage D only. They are not true of the run as a whole:
- **Stage A:** 1 `forge_build_ios`, ok.
- **Stage B:** 2 installs, both ok (the second reported `verified: false`), and 2 phone UI runs, both passed.
- **Stage C:** 2 phone UI runs, both failed. The journal passed all 29 steps but failed on screenshots; the next run lost the phone at step 5.

A record covering the whole run would need `forge verify --run 2026-11-08T18-06-43` over the earlier journals. That is a CLI command, and it wasn't approved in this session. The record makes no claim that contradicts a tool result I saw. It leaves out what happened before 18:36:31, and its "had no phone" reason is the Stage D server's, not the run's.

**The wider view** (`forge_verify` without `run`, which reads all 9 evidence bundles) does see the phone runs:
- **GD-7 device:** `passed`, run `2026-11-08T18-32-37`, **current: false**.
- **GD-1, GD-3 and GD-6 device:** `failed`, run `2026-11-08T18-34-56`, current: false. That is the journey run where the phone dropped at step 5, with light mode pinned, since reverted. The previous journey run (18:33:11) passed all 29 steps but failed its screenshot comparison.

The phone results don't count as current because the files changed after those runs. The test files and baselines were edited and then restored, and this report was written. The app's Swift and Rust have not changed since the build that passed GD-7 on the phone (installed 18:27). But Forge's rule decides "current", and by that rule GD-7 is not proven on the phone for this tree. A rerun of `uitests/guided.toml` would make it current.

### What is proven, and where

| | Requirements |
|---|---|
| **Proven on the phone, and counted by the record** | None |
| Seen passing on the phone, but not current for this tree | GD-7: `guided.toml` passed 36 of 36 twice (18:27:48 and 18:32:37), on Cati, iOS 26.3.1, in dark mode. No crash reports were found (`forge_read_crashes`, Stage B) |
| **Proven only on Linux** (Rust tests and the Swift model with the real Rust core) | GD-1 to GD-7 twin. GD-2, GD-4 and GD-5 need nothing more, so they are proven. The SwiftUI compiled with the Darwin SDK (`forge_build_ios`, and the two installs) |
| **Not proven** | GD-1, GD-3 and GD-6 on the phone: the journey's screenshots differ from the baselines, and nobody has looked at why. GD-7 on the phone for the current tree. Every requirement's acceptance by a person |

Also not proven:
- Light mode on the phone.
- The largest text size on the phone, which the owner left out.
- The audit after the timer fix.
- The exploratory walk.
- Leaving part way across a relaunch on the phone. The twin tests cover this.
- `scripts/check-bindings.sh` / `make validate`.

### Did the run find a genuine problem in its own work?

**Yes, one: the timer's Start and Stop buttons were too small to tap.** In Stage A I left them as bare text buttons. On the phone, `forge_ui_audit` (Stage B) measured `guided.timer.stop` at 59 x 20 pt. The guided session was built to be used while holding a lead, so this mattered. I fixed it with bordered buttons at least 44 pt, and the next screenshot showed the bigger button. The audit was not repeated, so the fix itself is seen but not audited.

Other problems were mistakes in the run's own process, not in the app:
- **Stage A:** a step count in my own parse test was wrong (39 instead of 36). I caught and fixed it on Linux.
- **Stage A:** glossary links inside the full-screen session would have tried to open from the tabs underneath. I found this by reading the code and fixed it before the phone; it has not been seen on the phone.
- **Stage C:** I misread the journey's screenshot failure as a light/dark mismatch, changed the test files and renamed the baselines. Then I checked the baseline image, found the hypothesis wrong, and reverted everything. That episode is also why the phone evidence is no longer "current".

The journey's screenshot differences on screens this change doesn't touch are still unexplained. That is a question for a person, not a problem the run has shown.
