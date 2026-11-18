# Device validation record

What each app has been shown to do on a physical iPhone, build by build. A green Linux check and a successful iOS build are not proof that an app is usable: this page is. It is the gate the [apps-first plan](roadmap.md#apps-first-prove-complete-then-extract) starts from, and each wave 3 PR adds a row for the app it moved.

Rules:

- A person fills in the device columns after a sitting, from what they saw on the phone ([AGENTS.md](../AGENTS.md), rule 8). An agent may fill in **Compiles** from a build it ran, and nothing else.
- **Yes**, **No** (with what went wrong in Problems) or **Not yet**. A skipped step is **Not yet**, never Yes.
- Record the build: the commit it was built from, and the date of the sitting. A later change to an app's Swift needs a new row; keep the old one.
- Problems lists layout, permission, lifecycle and crash problems seen, or "None seen". A problem that needs work links the PR or thread that takes it.

The questions, in order:

1. **Compiles**: the app builds for an iPhone (`scripts/check-ios-xtool.sh`, or `forge run ios --project examples/device-lab`, which builds before it looks for the phone).
2. **Installs**: the build installs on the iPhone (by cable with `forge run ios`; over-the-air installs are [parked](over-the-air.md)).
3. **Launches**: it opens from Device Lab's Apps screen without crashing.
4. **Main interactions**: the few things the app is for work by hand (listed per app below).
5. **Problems**: anything obviously wrong with layout, permissions or lifecycle (backgrounding, relaunch, locking the phone).

## Baseline: Device Lab from main after the 15 November merges

The first combined build of main after #226 and #227. Sketchbook, Soundscape and Relay R2 have never been run on an iPhone before this sitting.

| App | Main interactions to try | Build | Compiles | Installs | Launches | Main interactions | Problems | Sitting |
|---|---|---|---|---|---|---|---|---|
| Sketchbook | New sketch, draw with the tool picker, add a layer, export PNG and Share | | Not yet | Not yet | Not yet | Not yet | | |
| Soundscape | New project, record a take with the count-in, play it back with the metronome, mix two tracks | | Not yet | Not yet | Not yet | Not yet | | |
| Relay | Pair by QR code, send a note and a file, open the received file in Quick Look | Device Lab 1.29914001 (main 413e486) | Yes | Yes (cable) | Yes | Partly: sent a file to itself, and it arrived; opening one sheet straight after closing another worked ("Relay is working"). Pairing two devices not tried | None reported | 2026-11-16 |
| Pocket Library | Add a book, open the reader, read aloud, catalogue search | Device Lab 1.29914001 (main 413e486) | Yes | Yes (cable) | Yes | Partly: opened the reader and a summary; chapters can't be switched while a summary is open, which is fine. Read aloud and catalogue search not tried | None reported | 2026-11-16 |
| Commonplace | Add a feed, read an article in the reader, search articles, change a setting | Device Lab 1.29913703 (#245, db37cdb) | Yes | Yes (cable) | Yes | Yes: "looks good. Everything seems to be working" | None reported | 2026-11-16 |
| Chat | Join a room, send a message and a photo, open a notification | | Not yet | Not yet | Not yet | Not yet | | |
| Good Dog | Set up a dog, start a goal, open a lesson, see a safety rule hold a stage | | Not yet | Not yet | Not yet | Not yet | | |
| FieldKit | Record a short track, see it on the map, add a note, export GPX | Device Lab 1.29913703 (#245, db37cdb), then 1.29913748 (#250) | Yes | Yes (cable) | Yes | Yes: recorded and stopped a trip; the elevation and pace charts "look pretty good" (FK-2); GPX and GeoJSON export, rename and delete "all worked" (export checked: a 16-point track) | The Stop confirmation showed behind the recording screen; fixed in #250 (alerts on the stack in every app) and passed on Device Lab 1.29913748: "We are all good now" | 2026-11-16 |
| Checklist | Add, tick and search items | Device Lab 1.29914001 (main 413e486) | Yes | Yes (cable) | Yes | Partly: ticked items; Clear Done Items now asks first and works (#253). Search not tried | None reported | 2026-11-16 |
| Weather | Add a place, see its forecast | | Not yet | Not yet | Not yet | Not yet | | |

## Wave 3

One row per app as its move lands, before the next app starts ([roadmap](roadmap.md#apps-first-prove-complete-then-extract)).

| App | PR | Build | Compiles | Installs | Launches | Main interactions | Problems | Sitting |
|---|---|---|---|---|---|---|---|---|
| Weather | [#230](https://github.com/GeneralKnowledge/ixOS/pull/230) | Device Lab 1.29913589, main 59249ca + #230 (local merge 455a445) | Yes | Yes (cable) | Yes | Yes: current weather, add Prague | None reported (short check) | 2026-11-16 |
| Good Dog | [#239](https://github.com/GeneralKnowledge/ixOS/pull/239) | Device Lab 1.29913636 (04786c8), then 1.29913672 (89274ec, the reopen fix), main + #239 + #240 | Yes | Yes (cable) | Yes | Yes: opens and looks right; leaving Good Dog, opening Chat and opening Good Dog again shows the dog | Reopening showed a spinner on 1.29913636 (a new model on the open host skipped reading); fixed in #239 and passed on 1.29913672 | 2026-11-16 |
| Chat | [#240](https://github.com/GeneralKnowledge/ixOS/pull/240) | Device Lab 1.29913636, main + #239 + #240 (branch claude/project-thread-yz7ms9-gd-chat, 04786c8) | Yes | Yes (cable) | Yes | Yes: opens and looks right; with the chat server running, works (short check) | None reported for Chat. Live features need the chat server up | 2026-11-16 |

## 17 November sittings

Two Device Lab sittings, recorded from Reuben's reports (the sitting results files in the project's `iphone-session/` folder) and confirmed by him on PR #324. Results he gave as a general impression stay general here. Relay now holds Chat's messaging, so Chat has no row of its own. Apps with no result reported in these sittings (Sketchbook, Good Dog, Weather, FieldKit, Checklist) have no row.

| App | Build | Compiles | Installs | Launches | Main interactions | Problems | Sitting |
|---|---|---|---|---|---|---|---|
| Relay | Device Lab 1.29915393, then 1.29915595 | Yes | Yes (cable) | Yes | Yes, item by item: a folder Linux to iPhone (shown in Files and the history), photos iPhone to Linux, a 1 GB file resumed after Relay was swiped away (peak memory 74.6 MB); Chats: a message, a large document, a voice message, search, a notification, a burst of 15 across channels; trusted devices both ways with no code; New Messages pill and divider (#312) | New Messages pill failed on 1.29915393 (auto-scrolled, no pill); fixed in #312 and passed on 1.29915595 | 2026-11-17 |
| Soundscape | Device Lab 1.29915393, then 1.29915595 | Yes | Yes (cable) | Yes | Yes, item by item: Use This Recording, a title and album edit kept, the next song while locked, a new take records (#295), a loop stops on an interruption, lock screen details and song interruptions (#306) | Lock screen details and song interruptions failed on 1.29915393; fixed in #306 and passed on 1.29915595 | 2026-11-17 |
| Pocket Library | Device Lab 1.29915393, then 1.29915595 | Yes | Yes (cable) | Yes | Partly: "working fine" (a general report, not item by item); covers load and show offline (#298) | Read Aloud shows on the lock screen without details; fixed in #313, **recheck pending** | 2026-11-17 |
| Commonplace | Device Lab 1.29915393, then 1.29915595 | Yes | Yes (cable) | Yes | Yes: Undo after Mark All as Read (#294); article pictures show offline once seen (#298) | Pictures not kept offline on 1.29915393, which predates #298; passed on 1.29915595 | 2026-11-17 |
| Device Lab | Device Lab 1.29915393, then 1.29915595 | Yes | Yes (cable) | Yes | Yes, item by item: full screen and accents, the workshop menu, Apps-first home with This Build, Switch App, Tools (Logs, Servers, Checks sharing), This iPhone (battery drain, audio output and interruptions, network addresses, What iOS Keeps), Interaction Patterns with VoiceOver, AI Lab. Logs filter, search and Share not separately confirmed | The menu stayed visible briefly after Move This Button on 1.29915393; gone on 1.29915595 | 2026-11-17 |
