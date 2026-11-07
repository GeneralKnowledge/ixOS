# iPhone test session, 2026-11-07 (evening)

One ordered plan for every phone check that is queued after today's merges, run as **one coordinated session**: one Remote Control session on the laptop, driven by this plan's thread, so separate threads don't each ask Reuben to test one thing. Work top to bottom.

The order follows Reuben's review of 2026-11-07 18:42. **Part 1 (steps 1–4) settles the tooling questions** that the agent and MCP work depends on: Apple's accessibility service (#79), WebDriverAgent UI testing (#61), which team xtool signs with, and the launch/screenshot/diagnostics loop. **Part 2 (steps 5–8)** is the queued app checks, in the time that's left. Each step says **which app slot** it uses, **what Reuben does on the phone**, **what runs on the laptop** (through Remote Control, run by Claude, never typed by Reuben), and **the evidence** that decides pass or fail.

Nothing here is marked passed until Reuben reports it. Results go back to the runbook each step names (ACCEPTANCE tables, `local-ai.md`, the ledger), not into this file.

## The phone and the limits

- **Cati**: iPhone 15 Pro Max (iPhone16,2), iOS 26.3.1 (a), Developer Mode on, free Apple account.
- **Three sideloaded apps at most, on a free account.** The WebDriverAgent runner for UI testing counts as one. Free accounts can also register only 10 new app IDs per 7 days, so tonight's new apps (runner, Chat, Device Lab, Kataster) stay well within that if nothing else is tried. Reuben has not joined the paid Apple Developer Program yet and may decide to tonight; the slot plan below is the default, and [Paid team](#paid-team-install-side-by-side) is the branch to take if he does. Step 3 finds out which applies.
- **Storage is the blocker for the AI checks.** 496.79 of 512 GB used; the 2B model failed with 1.3 GB usable. iOS's "free" figure includes purgeable space, so the number that matters is the one the AI check app's `start` line prints (`free disk`).
- **Laptop**: Linux, xtool 1.24.0 (patched `-ixos.1`), Xcode 26 SDK, the ixOS checkout at `/home/red/ixOS`, Remote Control device "create". Keep that checkout untouched; laptop steps use worktrees.

## Slot plan (free account)

Expected now (step 0 confirms): **Pocket Library**, **AICheck** (it replaced ServicesCheck for the L0 run; models already copied in), **Weather**.

| Slot | Now | Session | After |
|---|---|---|---|
| A | Pocket Library | Stays. Updated in place once, from #61's branch (identifiers for the probe and UI tests, and the EPUB reader). Data is kept | Pocket Library |
| B | AICheck | Stays until the L0 run is finished, so the Qwen models already on the phone don't need copying again | Free (Device Lab or Kataster, optional) |
| C | Weather | Weather deleted → WDA runner (steps 2–4) → runner deleted → Chat | Chat |

The accessibility probe (step 1) needs no slot at all. Weather has nothing left to test (the redesign was confirmed on Cati and #52/#54 are merged); it can be reinstalled any time with `forge run ios --project examples/weather`.

## Paid team: install side by side

Reuben hasn't joined the paid program yet and may decide on the spot; step 3 checks. This branch applies only if he pays **and** xtool then signs with the paid team. What paying changes, to help decide:

| | Free Apple ID (now) | Paid Developer Program |
|---|---|---|
| Sideloaded apps | 3 at once | No such limit |
| Signing lasts | 7 days, then reinstall | 1 year (never longer than the membership) |
| Tonight | Weather deleted, runner deleted after step 4, Device Lab or Kataster only if AICheck goes | Everything stays installed; Device Lab and Kataster fit alongside |
| UI test runner | Reinstalled every week, takes a slot | Stays installed |
| Paid-only capabilities (from the [ledger](../capability-ledger.md) and [ios-signing.md](../ios-signing.md)) | Not available | Become possible later: push notifications (APNs), universal links, Sign in with Apple, Wi-Fi network name, NFC; multicast also needs Apple's approval |
| Not changed | Developer Disk Image still mounted after each reboot; phone storage is still the limit for the AI models; the check order is the same |

If the paid team is active: nothing gets deleted (Weather stays, AICheck keeps its models), the runner stays installed after step 4, Chat installs straight away in step 6, and Device Lab (installed in step 3) can be checked any time. Leave each app's `team_type` in `forge.toml` as it is tonight; moving apps to `"paid"` is a separate change.

## Who drives the laptop

**One Remote Control session for the whole evening**, started from this plan's thread, so two installs never fight over usbmuxd and Reuben allows one card. The other threads don't start their own sessions tonight: this thread runs their steps exactly as their runbooks say and posts each block's raw output to the thread that owns it (probe → "Speed up the Linux iOS dev loop", UI testing → "Automate testing on a real iPhone", Chat → "Grow Chat into flagship acceptance app" and "Extract shared infrastructure into ixOS", AI → "Plan on-device AI assistant for Chat"). Those threads interpret and record the results.

General rules for every laptop step: run commands yourself, report output verbatim, don't edit code mid-session, and if an install hangs, ask Reuben to replug and unlock (usbmuxd drops) and retry once.

---

## Step 0. Before anything (about 15 minutes)

**On the phone (Reuben):**

- [ ] Settings → General → iPhone Storage → Podcasts → **Remove Older Downloads** (frees up to 18.7 GB).
- [ ] Optionally delete a few YouTube offline videos for more margin. Target: **at least 10 GB usable**. Leave Offload Unused Apps **off** so iOS doesn't remove a test app.
- [ ] Settings → Apple Intelligence & Siri: on, and its model fully downloaded (needed for the Foundation Models checks in step 7).
- [ ] Charge to above 50% or keep it on the cable (Cati was at 45% earlier).
- [ ] Plug Cati into the laptop, unlock it, keep it unlocked. Allow the one Remote Control card.

**Laptop (Remote Control):**

- [ ] `cd /home/red/ixOS && git fetch origin`. Two worktrees: current `main` at `~/ixos-session`, and #61's branch (`claude/project-thread-e6ryft`) at `~/ixos-uitest`. `cargo build -p forge-cli` in both; use `~/ixos-uitest/target/debug/forge` for every `forge ui` command and the Pocket Library install.
- [ ] `forge doctor --json` and `forge device list --json`: record the iPhone, iOS version and **the installed apps**. If the three apps are not Pocket Library, AICheck and Weather, stop and tell Reuben before deleting anything.
- [ ] Find Xcode's `Contents/Resources/CoreDeviceDDIs` folder on the laptop (needed in step 1; xtool's SDK doesn't keep it); if only an `Xcode*.xip` exists, report its path and stop there (no long extractions).
- [ ] Download the WebDriverAgent runner zip and check its SHA-256 against the value in the UI-testing thread; stop if it differs.
- [ ] Start builds that don't need the phone, in the background, so later steps only install: Chat (`forge build ios --project examples/chat` from `~/ixos-session`) and the AICheck release build from the L0 thread (commit 7b8eb70 or later).

**Evidence:** the `doctor` and `device list` JSON; Reuben's storage screenshot after the clean-up.

---

# Part 1: the tooling questions

## Step 1. Apple's accessibility service probe (#79) — no app slot, about 15 minutes

Owner: "Speed up the Linux iOS dev loop" thread. What each result decides: [docs/research/ax-audit.md](../research/ax-audit.md). If it works, Forge can read the screen's accessibility tree, press controls and run Apple's audit **without installing the WebDriverAgent runner**.

**Laptop (Remote Control):**

- [ ] Install Pocket Library with the accessibility identifiers, in place, from `~/ixos-uitest`: `forge run ios --project examples/pocket-library`. (It also has the EPUB reader; the library is kept.)
- [ ] **Run 1, before the Developer Disk Image is mounted**, from `~/ixos-session`, with Pocket Library open on its library screen:
  `cargo run -p forge-device --features ax-probe --example ax_probe -- --max 40`
  `cargo run -p forge-device --features ax-probe --example ax_probe -- --press library.add`
  `cargo run -p forge-device --features ax-probe --example ax_probe -- --audit`
- [ ] Mount the image: `forge ui setup --ddi <CoreDeviceDDIs dir>` (needs internet for Apple's signing server). Report its output; it also lists the installed apps.
- [ ] **Run 2**: the same three commands again, Pocket Library back on its library screen.

**On the phone (Reuben):** keep Pocket Library open on the library screen, unlocked. After each `--press`, say whether the Add button visibly reacted (the New Book menu or form opened), then close what opened.

**Evidence:** the [1]–[4] lines of both runs; whether `Identifier=` shows the SwiftUI identifiers (`library.add` and so on); whether Add reacted; the audit output. Comparing run 1 and run 2 says whether the service needs the disk image. Raw output goes to the dev-loop thread.

---

## Step 2. WebDriverAgent UI testing M0 (#61) — slot C: Weather → runner, about 30 minutes

Owner: "Automate testing on a real iPhone" thread, PR #61 (draft, kept draft until this run; head bab78d4 at 19:00, check the thread for a newer one). The exact commands are in [forge-ui-testing.md](../forge-ui-testing.md) on that branch, under "Running M0" and "What still needs the phone"; this is the summary. Add `--json` to every `forge ui` command.

**On the phone (Reuben):**

- [ ] Delete **Weather** (touch and hold → Remove App → Delete App). If Reuben has already joined the paid program and switched xtool to it before the session, do step 3 first; with a paid team, Weather stays.
- [ ] If the runner won't start: Settings → General → VPN & Device Management → trust the developer profile.
- [ ] Keep the phone unlocked and don't touch the screen while the test runs.

**Laptop (Remote Control), all from `~/ixos-uitest`:**

- [ ] `forge device list` shows Weather gone, then `forge ui setup --runner ~/forge-uitest/WebDriverAgentRunner-Runner.app` (add `--team paid` only if step 3 already showed a paid team; it changes only the app-limit wording). If it fails at "starting WebDriverAgent", capture the full error.
- [ ] `forge ui run examples/pocket-library/uitests/m0.toml --out ~/forge-uitest/out`.
- [ ] `forge ui tree --app org.forgemobile.pocketlibrary` and `forge ui screenshot ~/forge-uitest/out/manual.png`.

**Pass:** the runner launches; M0 launches Pocket Library, taps `library.add`, types a title, sees the form react, takes screenshots and cancels; the screenshots are valid PNGs; `ui tree` lists the identifiers. The library is unchanged afterwards (Reuben glances at it).

**Evidence:** the `--json` output of every `forge ui` command and the whole output folder (sent to the UI-testing thread), `ls -la` and `file` on it, and what Reuben saw. Also put the WDA tree next to step 1's probe tree: do both show the same identifiers and labels? That comparison decides how much of UI inspection can drop the runner.

---

## Step 3. Which team does xtool sign with? — about 10 minutes, more if Reuben pays now

Owner: this thread; record the result in [ios-signing.md](../ios-signing.md) and the ledger's signing rows. Reuben does the sign-in himself; Apple credentials never go through Claude.

- [ ] **Reuben decides** whether to join the paid program tonight. If not, skip to the fourth-app test, which should be refused.
- [ ] If he joined: once Apple shows the membership as active, Reuben runs in a terminal on the laptop `xtool ds teams list`, then `xtool auth logout` and `xtool auth login`, picking the paid team. A new membership can take a while to become active; if the paid team isn't listed yet, carry on with the free plan and come back to this at the end.
- [ ] Laptop: `xtool auth status` and `forge doctor --json`.
- [ ] **Fourth-app test.** With Pocket Library, AICheck and the runner installed, `forge run ios --project examples/device-lab --json`.
  - **Free team:** expected to be refused with "maximum number of apps for free development profiles". Record the exact message and whether Forge's `--json` result names the app limit as the cause and suggests removing an app.
  - **Paid team:** Device Lab installs and opens. Then follow [Paid team](#paid-team-install-side-by-side) for the rest of the evening. Reinstalling Pocket Library (`forge run ios`) on the new team also shows whether an app signed by the old team needs reinstalling.

**Evidence:** the team list (team names only, no ids or emails in the thread), `xtool auth status`, the `--json` result of the fourth-app install, and Reuben's word on whether Device Lab opened.

---

## Step 4. The launch, screenshot and diagnostics loop — about 15 minutes

Owner: "Speed up the Linux iOS dev loop" thread ([dev-loop.md](../dev-loop.md)). One measured cycle on Pocket Library with tonight's real commands, to see what an agent would get back. No code changes.

**Laptop (Remote Control), with timestamps for each command:**

- [ ] `forge run ios --project examples/pocket-library --json` from `~/ixos-uitest` (rebuild and reinstall, no changes): the `--json` result object, exit code, and the time taken.
- [ ] Start `forge device logs --process AppHostXtool --for 30s --json` in the background.
- [ ] `forge ui run examples/pocket-library/uitests/m0.toml --out ~/forge-uitest/loop` (launches the app and takes screenshots), while the logs run.
- [ ] `forge ui tree --app org.forgemobile.pocketlibrary` and `forge ui screenshot ~/forge-uitest/loop/after.png`.
- [ ] `forge device crashes --app pocketlibrary --json`.

**On the phone (Reuben):** nothing, beyond keeping it unlocked and not touching it.

**Pass:** each command returns on its own without a hang; `device logs --for 30s` ends after about 30 s with one JSON object (#75); the logs include Pocket Library's Swift and Rust lines (ForgeLog, #66); crashes is empty.

**Evidence:** every command's output and time. The dev-loop thread uses it to say what the loop still lacks for an agent (for example one command that combines the logs, the crash list and a screenshot).

**Then (free team only):** Reuben deletes the runner (`XTL-…` app) to free slot C for Chat. `forge ui setup --runner` reinstalls it when needed; it expires after 7 days anyway.

---

# Part 2: queued app checks

## Step 5. Pocket Library reader and read-aloud (slot A, no change) — about 15 minutes

Owner: "Add EPUB reading to Pocket Library" thread, PR #53 (merged). Steps: [iphone-acceptance.md](../integration/iphone-acceptance.md) section E for the basics; the reader specifics below come from #53.

**Laptop:** step 1 installed Pocket Library from #61's branch, which already contains the reader (#53). Part 1 is done by now, so the identifiers are no longer needed tonight: if #82 or #83 (the extraction follow-ups) merged before the session, update `~/ixos-session` to main and reinstall Pocket Library from it, in place. Otherwise keep the #61 build. #83's Swift has not been compiled anywhere yet; if the build fails in `ReadAloud.swift`, that's the cause: report the error and keep the #61 build. Get Reuben's manuscript EPUB onto the phone (AirDrop or Files) if it isn't already there.

**On the phone (Reuben):**

- [ ] Optional first: Settings → Accessibility → Spoken Content → Voices, download an Enhanced or Premium voice.
- [ ] Import the manuscript EPUB again. **Pass:** it attaches to the existing book (same title and author) and keeps its progress; no duplicate book.
- [ ] Open it: chapters show, the contents sheet jumps between chapters, text size and the serif toggle work.
- [ ] Read a little, leave the book, reopen it. **Pass:** it opens where you left off, and the page count on the book moved.
- [ ] Tap the headphones: it reads aloud, highlights each word, scrolls along. Try pause, skip paragraph, speed and voice.
- [ ] Lock the phone while it reads. **Pass:** it keeps reading; play and pause work on the lock screen.
- [ ] *Only if #82 (files onto FileStore) is in the build:* books that had covers before the update still show them, and the reader still opens a chapter with pictures. Proves old saved file names still load on the device.
- [ ] *Only if #83 (read-aloud onto ForgeAudioSession) is in the build:* start some music in another app, then read-aloud in Pocket Library, lock the screen: speech carries on, lock-screen play/pause and skip work, and the other app's music resumes after Stop.

**Evidence:** pass/fail per line, one screenshot of the reader and one of the lock screen player; `forge device crashes --app pocketlibrary` empty; which commit was installed. #82/#83 results go to the "Finish the extraction follow-ups" thread.

---

## Step 6. Chat (slot C: Chat) — 45 to 90 minutes

Owners: "Grow Chat into flagship acceptance app" thread (M4, #60) and "Extract shared infrastructure into ixOS" thread (#64, #70). Runbook: [examples/chat/ACCEPTANCE.md](../../examples/chat/ACCEPTANCE.md).

Note: #60 merged before its by-hand Mac run finished. If that run found a Swift error, the Chat build on the laptop shows it; the Chat thread fixes it before this step.

**Laptop (Remote Control):**

- [ ] Start the server on the existing data file: `make chat-server ARGS="--bind 0.0.0.0:8787"` from `~/ixos-session`, and note the laptop's Wi-Fi IP (it was 192.168.1.169).
- [ ] `forge run ios --project examples/chat --json` (the second real-device `--json` check: one result object, exit code 0).
- [ ] `forge device logs --process ChatXtool --json` kept running in the background for the whole step, saved to a file.

**On the phone (Reuben), in this order:**

- [ ] Open Chat. Allow Local Network and Notifications. If it asks to sign in, sign up with `http://<laptop IP>:8787` (Keychain may keep the old account; either is fine). If you see "Host is down", check Settings → Privacy & Security → Local Network → Chat.
- [ ] **Notification tap (#70).** Press Home. Laptop sends `make chat-cli ARGS="send random tap test"`. Tap the banner. **Pass:** Chat opens straight into Random, not the Chats list. Repeat once from the lock screen.
- [ ] **Attachments warning (#64).** Send one photo in General (+, Photo). Laptop: grep the saved log for `WARN [Attachments]`. **Pass:** no such line. If there is one, copy it verbatim.
- [ ] **Voice messages H1–H12 (M4).** Follow the M4 table in ACCEPTANCE.md. Have headphones for H8; H11 needs someone to call the phone (skip and say so if nobody can). H12 (release build) is a laptop step at the end.
- [ ] **If there is time: photos and documents G1–G11 (M3).** G3 and G7 are laptop steps (`chat-cli download`/`send-file`); have a PDF in Files.
- [ ] **Optional: M2 F1–F14.** Reuben said to assume these work; run them only if the evening allows, together with M3.

**Evidence:** pass/fail per row id (H1…, G1…), the `--json` output, the log file and the grep result, and the run details the runbook asks for (iPhone, iOS, xtool version, commit).

---

## Step 7. Local AI L0, remaining checks (slot B, AICheck) — about 45 minutes

Owner: "Plan on-device AI assistant for Chat" thread, #55 (merged). Runbook: [docs/local-ai-check.md](../local-ai-check.md). Already done: K1, K2, Qwen3.5 0.8B load and debug speed, the Swift→Rust push cost, Mia for three turns.

**Laptop (Remote Control), before Reuben touches the app:**

- [ ] List the app's `tmp/` and `Documents/` with sizes. Delete every `*.mlmodelc` left in `tmp/` by the failed 2B loads.
- [ ] Install the fixed **release** build (7b8eb70 or later) **over** the existing app, without uninstalling, so the Qwen models stay. Confirm both Qwen folders are intact.
- [ ] Keep `forge device logs --grep ai-check | tee ~/ai-check-results.txt` running.

**On the phone (Reuben), in this order:**

- [ ] K1: open the app. Read out `free disk` from the `start` line. **Needs about 5 GB for the 2B (K13).** If it's lower, go back to step 0's clean-up before K13.
- [ ] K3, K4: Foundation Models generate, then auto-stop after 20 pieces.
- [ ] K8: Qwen3.5 0.8B load, generate, Stop halfway, then auto-stop. **Pass only if** `stop took` is under a second and `late pieces` is 0. Note pieces/s (release build; debug was about 9).
- [ ] K7: unload and load again. Is the second load fast (compiled model reused)?
- [ ] K9: Mia then Leo, reminder on, ten messages each. Note per reply whether it stayed in character.
- [ ] K10: reminder off, five messages. K11: Foundation Models, five messages.
- [ ] K12: force-quit, airplane mode and Wi-Fi off, open, load 0.8B, generate.
- [ ] K13: Wi-Fi back on, Qwen3.5 2B load and generate (only with about 5 GB free).
- [ ] K15: after a few minutes of generation, Record memory.
- [ ] Share results (or the laptop keeps `~/ai-check-results.txt`).
- [ ] Optional: K5, K6 (LFM2.5 350M).

**Evidence:** the result lines; on a K13 crash, `forge device crashes` (a jetsam report means a memory limit, a compile error -14 means disk).

---

## Step 8. Optional, only if the evening allows (slot B after L0, or slot C)

Free a slot by deleting AICheck **only after Reuben confirms the L0 results are recorded**; the models would have to be copied again.

- **Device Lab** (#59, merged; never run on a phone). `forge run ios --project examples/device-lab`. Phone checks are in [examples/device-lab/README.md](../../examples/device-lab/README.md#checking-it-on-an-iphone), items 1–7 (airplane and Wi-Fi changes, Bluetooth, shake, compass, barometer floor-to-ceiling, walk 100 m, battery and Low Power Mode). Most can be done indoors except the 100 m walk.
- **Kataster** (Bevy trial, #62 merged; low priority per Forge's focus). Laptop: `xtool install ~/kataster-trial/ixos-trial/examples/kataster-ios/Kataster.app`. Phone checks are in [bevy-game-trial.md](../bevy-game-trial.md) step 4: menu and Play, joystick and fire, sound, whole arena in landscape, paused after 30 s in the background, no crash in 5 minutes, frame rate from `forge device logs --process kataster`.

Pick at most one tonight; Device Lab first, since it exercises framework services other apps will use.

---

## End of session

- [ ] Phone ends with: **Pocket Library, Chat, AICheck** (or whichever app step 8 put in its place). With a paid team: everything installed tonight stays.
- [ ] Laptop: stop the chat server, keep the worktrees and logs, end the Remote Control session.
- [ ] Each owning thread records its results: the accessibility probe in `docs/research/ax-audit.md`, UI testing in #61, the team result in `ios-signing.md` and the ledger, Chat in ACCEPTANCE.md M4 (and M3/M2 if run), the #64/#70 checks in the extraction thread, L0 in `local-ai.md` and the ledger, the reader in Pocket Library's acceptance notes, `--json` on device in `dev-loop.md`.
