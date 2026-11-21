# App UI review, November 2026

**Status:** review, 2026-11-21. Reuben asked: "We should also look through the apps and see where we should be using [ForgeKit]. Lots of our UI will be basic or half done." This review reads the source of every app on `main` plus #363. It doesn't repeat the [ForgeUI coverage audit](forgeui-coverage.md) (#325, three days ago) or the [experience audit](audit.md). It checks which of their findings are still true, and adds what they didn't cover. Nothing here was seen on the phone: findings are from the code.

## The answer

- **Mostly the apps are complete but plain, not half built.** The code has no placeholders, TODOs, or raw errors shown to people. Every list has its empty, loading and failed states (`ForgeStateView` is in every app except Sums, which has no lists). What makes the UI feel basic:
  - Every row and header is hand-built from `.font(.headline)` plus `.secondary`: 453 raw `.font(` calls against 14 uses of text roles.
  - No app has an icon.
  - A few screens open with a table of values where a header belongs.
- **Three things are hand-built in three or more apps, and belong in ForgeKit** (the "second use" rule is met several times over):
  1. Export a file, then share it: 4 apps.
  2. Rename: 5 places in 4 apps.
  3. An inline notice, such as "offline" or "the map can't update": 4 apps.
- **One app still has another app's name:** Relay absorbed Chat (#296), but about 15 messages people read still say "Chat" ("Welcome to Chat", "Sign out of Chat?", "allow Chat to use the microphone"), and its Settings tab is still Chat's.

## Still true from the earlier audits

| Finding | Then | Now |
|---|---|---|
| X1 no app icon | 0 of 10 | **0 of 12**: no `icon` in any `forge.toml` |
| Text roles unused (A1) | 0 apps | Checklist 1 footer, Device Lab 13. The decision stands: no mass migration, roles come in when a screen is touched |
| Relay and Soundscape on the standard accent (D3) | Deferred to their redesigns | Unchanged, as decided |
| X11 swipe-only actions | 6 apps | 7 screens: Checklist rows, FieldKit trips and notes, Commonplace timeline (2), Soundscape playlists, Relay trusted devices and muted chats |
| X12 rename is an alert | 4 apps | Sketchbook (sketch, layer), FieldKit (trip), Relay (this device); Checklist uses a sheet |
| X5, S2 fixed point sizes | Several | Soundscape transport 64 pt (`ListenView.swift:846`, `:889`) and take rows 34 pt; Weather's bottom bar 19 pt (`WeatherView.swift:126`). Sums' 72 pt display and Good Dog's 96 pt celebration are display sizes and fine |
| X15 bare root spinner | 3 apps | Fixed at the root (`root-loading` rule). Inline bare `ProgressView()` remains in 15 places, which is fine inside a row or button |
| C3 notice component | Proposed | Not built; now 4 users (below) |

Fixed since: Checklist is on ForgeUI with its accent and state views (#331), app sheets moved onto ForgeUI's swipe guard (#331), the mini-player and Undo banner sit in `forgeBar` (#322, #363), haptics are everywhere (#309).

## What ForgeKit should gain

Each of these moves every app that has it in the same PR (rule 12), with a parity rule so new screens use it.

**1. Export and share a file (4 apps).**
- Each app has its own sheet that shows a file and a `ShareLink`:
  - Checklist `ShareSheet` (`ChecklistView.swift:338`);
  - FieldKit `ShareSheet` (`TripDetailView.swift:139`);
  - Commonplace `ExportSubscriptionsView` (`SubscriptionFileViews.swift:162`);
  - Sketchbook `ExportSheet` (`ExportSheet.swift:12`, which also chooses a format).
- Pocket Library does the same inline in Settings.
- Three of them define the same `ExportedFile`/`ExportFile` identifiable wrapper.
- **Proposal:** `.forgeShareFile($file)` in ForgeAppKit takes an export that may fail (through `ForgeSubmission`) and shows the system share sheet with the file once it's ready. The extra "your file is ready" step goes.

**2. Rename (4 apps, 5 places).**
- An alert with a text field can't show why a name was refused, so a bad name fails after the alert has closed.
- Checklist built a sheet to get round that (A39 in the friction log).
- **Proposal:** `ForgeRename` (C2 in the coverage audit): a small sheet on `forgeFormToolbar` that shows Rust's refusal under the field. Moves Sketchbook ×2, FieldKit and Relay, and Checklist's own sheet.

**3. Inline notice (4 apps).**
- Weather's `Notice` card (`ForecastPage.swift:174`).
- Relay's `ConnectionBanner` (`ChatView.swift:666`) and its account problem in Settings.
- FieldKit's location and map warnings on the recording screen (`RecordingView.swift:37-56`).
- Commonplace's `RefreshStatus` (`FeedListView.swift:251`).
- Each picks its own icon, size and colour for the same message: something is wrong, here is what still works, here is the one action.
- **Proposal:** `ForgeNotice(tone:, message:, action:)` in ForgeUI (C3). Weather keeps its sky colours through the tone.

Not proposed: a shared stat tile. FieldKit's `Stat` and Weather's `Tile` look alike, but Weather's is part of its own design. Pocket Library's `StatRow` is a list row. Wait for a second plain use, such as Good Dog's progress charts (GD4).

## App by app

Where an existing ForgeKit piece fits, and what looks unfinished.

**Relay** (its own product track: these go to the Relay thread, not here).
- The Settings tab is Chat's:
  - it, and about 15 other messages across the app, call the app "Chat" (sign-up, sign-out, app lock, microphone permission, errors);
  - it shows developer details (Account ID, storage engine) as the first section;
  - "None" is a row;
  - Unmute is swipe-only with no identifier;
  - the file doesn't import ForgeUI.
- `TrustBadge` is `ForgeStatus("Trusted", tone: .success)` by hand.
- Standard accent (D3).

**Good Dog.**
- My Dog opens with the dog's name as a section header over four `LabeledContent` rows. `ForgeDetailHeader` (name, life stage, focus as a badge) is what it was made for, and has no users yet.
- Goal rows hand-build title, stage and a warning line. A `ForgeRow` with a `ForgeBadge` for Paused or Set Aside fits.
- Today is a plain list of five sections; G3 (the plan is the third section) is still open.

**FieldKit.**
- Trip detail: rename alert (→ `ForgeRename`), share sheet (→ share file).
- An empty Notes section shows a grey sentence instead of an Add Note button.
- Trip and note delete are swipe-only.
- Trip rows have no map thumbnail (K3), so every trip looks the same.

**Checklist.**
- Swipe-only delete and rename.
- Import results and errors use raw `.alert` (→ `forgeAlert`).
- The share sheet (→ share file).
- Otherwise plain by design until due dates land (the app plan, #358).

**Sketchbook.**
- Two rename alerts.
- Hard-coded corner radius 12 on tiles (→ `ForgeRadius.m`).

**Soundscape.**
- 64 pt transport and 34 pt take buttons don't follow Dynamic Type (S2).
- Playlists are swipe-only.
- Standard accent (D3).

**Commonplace.**
- Timeline swipe actions have no menu twin.
- `RefreshStatus` (→ notice).

**Weather.**
- Its own sky design is deliberate. Only the notice is shared.
- The 19 pt bottom bar.

**Pocket Library, Urdu Naturally, Sums.**
- Nothing new. Pocket Library is the most complete user of ForgeAppKit.
- Urdu Naturally, the newest app, is already built on `ForgeRow`, `ForgeButton` and `ForgeBadge`: new apps pick ForgeUI up when it's there.

## Proposed order

1. **App icons, all apps.** The most visible unfinished thing, and no code. One 1024 px PNG per app in `forge.toml`. The images need Reuben's eye. Device Lab's icon is the one he sees every day.
2. **`ForgeNotice`, `ForgeRename` and share-file in ForgeKit**, one PR each, each moving its apps and adding a parity rule. They land after the ForgeKit 0.7 release and go into 0.8.
3. **Swipe actions get a menu twin**, the 7 screens, with a parity rule (a file with `.swipeActions` and no `.contextMenu` fails unless listed).
4. **Good Dog's My Dog header and goal rows** onto `ForgeDetailHeader` and `ForgeRow`: the first real users of both.
5. **Text roles screen by screen** as each screen is touched, as decided in #325. No sweep.
6. **Relay's Settings tab** goes to the Relay thread as a finding for its next pass.

Phone: steps 2 to 4 each need a short look in the combined Device Lab build: a notice, a rename with a refused name, a share, and a menu on a long press, in one app each.
