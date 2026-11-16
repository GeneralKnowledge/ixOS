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
| Relay | Pair by QR code, send a note and a file, open the received file in Quick Look | | Not yet | Not yet | Not yet | Not yet | | |
| Pocket Library | Add a book, open the reader, read aloud, catalogue search | | Not yet | Not yet | Not yet | Not yet | | |
| Commonplace | Add a feed, read an article in the reader, search articles, change a setting | | Not yet | Not yet | Not yet | Not yet | | |
| Chat | Join a room, send a message and a photo, open a notification | | Not yet | Not yet | Not yet | Not yet | | |
| Good Dog | Set up a dog, start a goal, open a lesson, see a safety rule hold a stage | | Not yet | Not yet | Not yet | Not yet | | |
| FieldKit | Record a short track, see it on the map, add a note, export GPX | | Not yet | Not yet | Not yet | Not yet | | |
| Checklist | Add, tick and search items | | Not yet | Not yet | Not yet | Not yet | | |
| Weather | Add a place, see its forecast | | Not yet | Not yet | Not yet | Not yet | | |

## Wave 3

One row per app as its move lands, before the next app starts ([roadmap](roadmap.md#apps-first-prove-complete-then-extract)).

| App | PR | Build | Compiles | Installs | Launches | Main interactions | Problems | Sitting |
|---|---|---|---|---|---|---|---|---|
| Weather | [#230](https://github.com/GeneralKnowledge/ixOS/pull/230) | Device Lab 1.29913589, main 59249ca + #230 (local merge 455a445) | Yes | Yes (cable) | Yes | Yes: current weather, add Prague | None reported (short check) | 2026-11-16 |
| Good Dog | [#239](https://github.com/GeneralKnowledge/ixOS/pull/239) | Device Lab 1.29913636 (04786c8), then 1.29913672 (89274ec, the reopen fix), main + #239 + #240 | Yes | Yes (cable) | Yes | Yes: opens and looks right; leaving Good Dog, opening Chat and opening Good Dog again shows the dog | Reopening showed a spinner on 1.29913636 (a new model on the open host skipped reading); fixed in #239 and passed on 1.29913672 | 2026-11-16 |
| Chat | [#240](https://github.com/GeneralKnowledge/ixOS/pull/240) | Device Lab 1.29913636, main + #239 + #240 (branch claude/project-thread-yz7ms9-gd-chat, 04786c8) | Yes | Yes (cable) | Yes | Yes: opens and looks right; with the chat server running, works (short check) | None reported for Chat. Live features need the chat server up | 2026-11-16 |
