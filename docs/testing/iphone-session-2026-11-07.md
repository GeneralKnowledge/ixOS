# iPhone test session, 2026-11-07 (evening)

One ordered plan for every phone check that is queued after today's merges, so threads don't take turns grabbing the phone. Work top to bottom. Each step says **which app slot** it uses, **what Reuben does on the phone**, **what runs on the laptop** (through Remote Control, run by Claude, never typed by Reuben), and **the evidence** that decides pass or fail.

Nothing here is marked passed until Reuben reports it. Results go back to the runbook each step names (ACCEPTANCE tables, `local-ai.md`, the ledger), not into this file.

## The phone and the limits

- **Cati**: iPhone 15 Pro Max (iPhone16,2), iOS 26.3.1 (a), Developer Mode on, free Apple account.
- **Three sideloaded apps at most, on a free account.** The WebDriverAgent runner for UI testing counts as one. Free accounts can also register only 10 new app IDs per 7 days, so tonight's new apps (runner, Chat, Device Lab, Kataster) stay well within that if nothing else is tried. Reuben has not joined the paid Apple Developer Program yet and may decide to tonight; the slot plan below is the default, and [Paid team](#paid-team-install-side-by-side) is the branch to take if he does. Step 0 finds out which applies.
- **Storage is the blocker for the AI checks.** 496.79 of 512 GB used; the 2B model failed with 1.3 GB usable. iOS's "free" figure includes purgeable space, so the number that matters is the one the AI check app's `start` line prints (`free disk`).
- **Laptop**: Linux, xtool 1.24.0 (patched `-ixos.1`), Xcode 26 SDK, the ixOS checkout at `/home/red/ixOS`, Remote Control device "create". Keep that checkout untouched; laptop steps use worktrees.

## Slot plan

Expected now (step 0 confirms): **Pocket Library**, **AICheck** (it replaced ServicesCheck for the L0 run; models already copied in), **Weather**.

| Slot | Now | Session | After |
|---|---|---|---|
| A | Pocket Library | Stays. Updated in place twice (UI test identifiers, then reader + read-aloud from main). Data is kept | Pocket Library |
| B | AICheck | Stays until the L0 run is finished, so the Qwen models already on the phone don't need copying again | Free (Device Lab or Kataster, optional) |
| C | Weather | Weather deleted → WDA runner (UI testing M0) → runner deleted → Chat | Chat |

Why this order: UI testing M0 and the Chat checks feed the MCP and agent work, so they go first. They share slot C, one after the other, and neither touches slot B, so the AI models survive. Weather has nothing left to test (the redesign was confirmed on Cati and #52/#54 are merged); it can be reinstalled any time with `forge run ios --project examples/weather`.

## Paid team: install side by side

Only if Reuben joins the program **and** step 0 showed a fourth app installs. A new membership can take a while to become active, and xtool must be signed in with that Apple ID; until both are true, use the slot plan.

What paying changes, to help decide:

| | Free Apple ID (now) | Paid Developer Program |
|---|---|---|
| Sideloaded apps | 3 at once | No such limit |
| Signing lasts | 7 days, then reinstall | 1 year |
| Tonight | Weather deleted, runner deleted after M0, Device Lab or Kataster only if AICheck goes | Everything stays installed; Device Lab and Kataster fit alongside |
| UI test runner | Reinstalled every week, takes a slot | Stays installed |
| Paid-only capabilities (from the [ledger](../capability-ledger.md)) | Not available | Become possible later: push notifications (APNs), universal links, Wi-Fi network name, NFC; multicast also needs Apple's approval |
| Not changed | Developer Disk Image still mounted after each reboot; phone storage is still the limit for the AI models; the check order is the same |

Then:

- **Nothing gets deleted.** Weather stays, AICheck keeps its models, and the WDA runner, Chat and Device Lab go on beside them.
- **Step 1:** skip "Delete Weather" and "delete the runner". Keep the runner installed for later UI test runs. With a paid team the signing lasts a year instead of seven days, but the Developer Disk Image still has to be mounted after every reboot.
- **Step 3:** Chat installs straight away; no slot to free.
- **Step 5:** Device Lab is already installed (step 0), so its checks can run any time. Kataster can be installed too.
- **The order of steps 1 to 4 stays the same**, because it follows priority (what feeds the MCP and agent work first), not slots.
- Leave each app's `team_type` in `forge.toml` as it is. Moving apps to `"paid"` (push notifications, associated domains and so on) is a separate change, not part of tonight.

## Who drives the laptop

One Remote Control session at a time, so two installs never fight over usbmuxd. Recommended: this plan's thread drives the whole session through one Remote Control card and posts each block's results to the thread that owns it. If Reuben prefers each thread to run its own block, hand over between blocks and close the previous Remote Control session first.

General rules for every laptop step: run commands yourself, report output verbatim, don't edit code mid-session, and if an install hangs, ask Reuben to replug and unlock (usbmuxd drops) and retry once.

---

## Step 0. Before anything (about 15 minutes)

**On the phone (Reuben):**

- [ ] Settings → General → iPhone Storage → Podcasts → **Remove Older Downloads** (frees up to 18.7 GB).
- [ ] Optionally delete a few YouTube offline videos for more margin. Target: **at least 10 GB usable**. Leave Offload Unused Apps **off** so iOS doesn't remove a test app.
- [ ] Settings → Apple Intelligence & Siri: on, and its model fully downloaded (needed for the Foundation Models checks K3, K4, K11).
- [ ] Charge to above 50% or keep it on the cable (Cati was at 45% earlier).
- [ ] Plug Cati into the laptop, unlock it, keep it unlocked. Allow the Remote Control card.

**Laptop (Remote Control):**

- [ ] `cd /home/red/ixOS && git fetch origin` and create a worktree of current `main` at `~/ixos-session` (`git worktree add ~/ixos-session origin/main`). `cargo build -p forge-cli` there.
- [ ] `forge doctor --json` and `forge device list --json`: record the iPhone, iOS version and **the installed apps**. If the three apps are not Pocket Library, AICheck and Weather, stop and tell Reuben before deleting anything.
- [ ] **Which team signs?** `xtool auth status` only says logged in or out, so ask Reuben which Apple ID xtool is signed in with, and whether it's the one enrolled in the paid program. If it isn't, he signs in again with `xtool auth login` in a terminal himself (credentials never go through Claude).
- [ ] **Prove it with a fourth app.** Install Device Lab (`forge run ios --project examples/device-lab`) without removing anything. If it installs and launches, the paid team is active: follow [Paid team](#paid-team-install-side-by-side). If xtool refuses because of the app limit, it's still the free team: carry on with the slot plan, and Device Lab waits for step 5.
- [ ] Start builds that don't need the phone, in the background, so later steps only install: Chat (`forge build ios --project examples/chat`), and the AICheck release build from the L0 thread (commit 7b8eb70 or later).

**Evidence:** the `doctor` and `device list` JSON; Reuben's storage screenshot after the clean-up.

---

## Step 1. UI testing M0 (slot C: Weather → runner) — about 30 minutes

Owner: "Automate testing on a real iPhone" thread, PR #61 (draft). The exact laptop script is in that thread's Remote Control instructions; summary below.

**Before the session:** #61's branch was cut before #53 merged. **Merge main into #61 first**, otherwise installing Pocket Library from it would replace the reader build with an older one. If that hasn't happened, install Pocket Library from main in step 2 right after M0.

**On the phone (Reuben):**

- [ ] Delete **Weather** (touch and hold → Remove App → Delete App).
- [ ] If the runner won't start, Settings → General → VPN & Device Management → trust the developer profile.
- [ ] Keep the phone unlocked and don't touch the screen while the test runs.

**Laptop (Remote Control):**

- [ ] Worktree of #61's branch at `~/ixos-uitest`, `cargo build -p forge-cli`.
- [ ] Download the WebDriverAgent runner zip and check its SHA-256 against the value in the thread; stop if it differs.
- [ ] *Optional, accessibility audit probe (PR #79), run 1 of 2:* before the disk image is mounted, with Pocket Library open and the phone unlocked, run from the #79 branch (`claude/project-thread-y2svc9`, or main once it merges):
  `cargo run -p forge-device --features ax-probe --example ax_probe -- --max 40`, then `-- --press library.add`, then `-- --audit`. Reuben only keeps Pocket Library in front and says whether the Add button visibly reacted to `--press`. The Pocket Library on the phone may not have the identifiers yet at this point, so judge `Identifier=` on run 2.
- [ ] Find Xcode's `CoreDeviceDDIs` folder; `forge ui setup --ddi <dir>` (mounts the Developer Disk Image, needs internet for Apple's signing server).
- [ ] `forge ui setup --runner …/WebDriverAgentRunner-Runner.app` (only after `device list` shows Weather gone).
- [ ] `forge run ios --project examples/pocket-library` (updates in place, keeps the library).
- [ ] *Optional, probe run 2 of 2:* the same three `ax_probe` commands again, after `forge run ios --project examples/pocket-library` below (so the identifiers are installed) and with the disk image mounted. Comparing the two runs shows whether Apple's accessibility audit service needs it.
- [ ] `forge ui run examples/pocket-library/uitests/m0.toml --out ~/forge-uitest/out`, then `forge ui tree --app org.forgemobile.pocketlibrary` and `forge ui screenshot ~/forge-uitest/out/manual.png`.

**Pass:** the runner launches; the M0 test opens New Book, types a title, sees the form react, cancels; screenshots are valid PNGs; `ui tree` lists Pocket Library's identifiers (`library.add` and so on). The library is unchanged afterwards (Reuben glances at it).

**Evidence:** full output of every `forge ui` command; for the optional probe, the [1]–[4] lines of each run, whether `Identifier=` shows the SwiftUI identifiers, and whether Add reacted to `--press` (send these to the "Speed up the Linux iOS dev loop" thread; [docs/research/ax-audit.md](../research/ax-audit.md) on #79 says what each result decides), `ls -la` and `file` on the output folder, and what Reuben saw on screen.

**Then:**

- [ ] Laptop: `forge device logs --process AppHostXtool --for 30s --json` while Reuben opens Pocket Library. This is the real-device check of #75 (`--json` result object, and `--for` ending on time even if the phone is quiet). Pass: one JSON object on stdout, exit code 0, ends after about 30 s.
- [ ] Reuben: delete the runner (`XTL-…` app) to free slot C. `forge ui setup --runner` reinstalls it later; it expires after 7 days anyway.

---

## Step 2. Pocket Library reader and read-aloud (slot A, no change) — about 15 minutes

Owner: "Add EPUB reading to Pocket Library" thread, PR #53 (merged). Steps: [iphone-acceptance.md](../integration/iphone-acceptance.md) section E for the basics; the reader specifics below come from #53.

**Laptop:** if step 1 installed a build without the reader, `forge run ios --project examples/pocket-library` from `~/ixos-session` (main). Get Reuben's manuscript EPUB onto the phone (AirDrop or Files) if it isn't already there.

**On the phone (Reuben):**

- [ ] Optional first: Settings → Accessibility → Spoken Content → Voices, download an Enhanced or Premium voice.
- [ ] Import the manuscript EPUB again. **Pass:** it attaches to the existing book (same title and author) and keeps its progress; no duplicate book.
- [ ] Open it: chapters show, the contents sheet jumps between chapters, text size and the serif toggle work.
- [ ] Read a little, leave the book, reopen it. **Pass:** it opens where you left off, and the page count on the book moved.
- [ ] Tap the headphones: it reads aloud, highlights each word, scrolls along. Try pause, skip paragraph, speed and voice.
- [ ] Lock the phone while it reads. **Pass:** it keeps reading; play and pause work on the lock screen.

**Evidence:** pass/fail per line, one screenshot of the reader and one of the lock screen player; `forge device crashes --app pocketlibrary` empty.

---

## Step 3. Chat (slot C: Chat) — 45 to 90 minutes

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

## Step 4. Local AI L0, remaining checks (slot B, AICheck) — about 45 minutes

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

## Step 5. Optional, only if the evening allows (slot B after L0, or slot C)

Free a slot by deleting AICheck **only after Reuben confirms the L0 results are recorded**; the models would have to be copied again.

- **Device Lab** (#59, merged; never run on a phone). `forge run ios --project examples/device-lab`. Phone checks are in [examples/device-lab/README.md](../../examples/device-lab/README.md#checking-it-on-an-iphone), items 1–7 (airplane and Wi-Fi changes, Bluetooth, shake, compass, barometer floor-to-ceiling, walk 100 m, battery and Low Power Mode). Most can be done indoors except the 100 m walk.
- **Kataster** (Bevy trial, #62 merged; low priority per Forge's focus). Laptop: `xtool install ~/kataster-trial/ixos-trial/examples/kataster-ios/Kataster.app`. Phone checks are in [bevy-game-trial.md](../bevy-game-trial.md) step 4: menu and Play, joystick and fire, sound, whole arena in landscape, paused after 30 s in the background, no crash in 5 minutes, frame rate from `forge device logs --process kataster`.

Pick at most one tonight; Device Lab first, since it exercises framework services other apps will use.

---

## End of session

- [ ] Phone ends with: **Pocket Library, Chat, AICheck** (or whichever app step 5 put in its place). With a paid team: everything installed tonight stays.
- [ ] Laptop: stop the chat server, keep the worktrees and logs, end the Remote Control session.
- [ ] Each owning thread records its results: UI testing in #61, Chat in ACCEPTANCE.md M4 (and M3/M2 if run), the #64/#70 checks in the extraction thread, L0 in `local-ai.md` and the ledger, the reader in Pocket Library's acceptance notes, `--json` on device in `dev-loop.md`.
