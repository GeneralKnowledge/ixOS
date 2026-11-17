# ixOS experience audit, phase 1

Status: 2026-11-17, on main at `15bc21d` (after #282). Part of [ixOS Experience Research](README.md). Research only: no app, ForgeUI or ForgeAppKit change is made here.

## The short version

The apps are better built than they are designed. Their states, errors and accessibility basics are mostly right, because the framework and the parity checks made them right. What is missing is the layer above: deliberate identity, a clear first minute, and the fastest path for the task people do most.

Most of this audit is **source verified** (see [evidence levels](#evidence-levels)): it says what the code does. Whether a new user is confused, or a screen feels satisfying, is a judgement until someone performs the workflow; those parts are marked as judgements and become findings in phase 5.

The eight findings that matter most:

1. **Nothing gives an app its own identity.** Our own rule says identity comes from the accent colour, the app icon, symbols and copy ([ios-ui.md](../../ios-ui.md)). No app has a configured custom icon: `icon` is commented out in five `forge.toml` files and absent from the rest. Every app that sets a theme uses `.forgeTheme(.standard)`, the same blue; Chat, Checklist, Weather and Device Lab set no theme at all. *Source verified.*
2. **The first minute asks for infrastructure.** Chat's first screen asks for a server address and a `make chat-server` command. Soundscape's Listen asks once for a take server. Commonplace starts empty and needs a feed URL; a site address fails. Relay starts with "No Devices" and a QR code. Good Dog is the exception: it asks about your dog and gives you today's plan. *Source verified; that it confuses a new user is a judgement.*
3. **The likely most common task is not the most prominent.** Pocket Library's "Reading Now" lives in the Activity tab, so continuing a book is Library, find it, open it, Continue Reading. Commonplace's article screen has no next article. Device Lab's Apps, the reason it is on the phone, is a toolbar icon above a long sensor dashboard. *Tap paths source verified; which task is most common is our assumption, not measured.*
4. **Recovering from a mistake means a confirmation, never an undo.** Deletes are either confirmed (Pocket Library, FieldKit, Sketchbook, Chat) or immediate and permanent (Checklist's swipe to delete). No app offers undo after the fact, outside Sketchbook's canvas. iOS's own answer for frequent, cheap actions is undo or Recently Deleted; for rare, costly ones it is a confirmation. We use one answer for both. *Source verified.*
5. **Settings live in a different place in each app.** A tab in Chat and Pocket Library, a sheet from a gear in Commonplace, a sheet inside Listen in Soundscape, nowhere in the rest. That is not wrong in itself; placement should follow each app's information architecture. What's missing is a reason for each choice. *Source verified.*
6. **Feedback is good in the states and thin in the moments.** Empty, loading and failed states exist almost everywhere (`ForgeStateView` or `ContentUnavailableView`). Haptics appear 19 times in seven apps (20 in eight projects counting the starter), animation in a handful, and most saves are silent. Feedback should be proportional to the action, so this is about the few moments that deserve it, not every tap. *Counts source verified; "rarely satisfying" is a judgement.*
7. **Accessibility is strong where an app was reviewed, unproven elsewhere.** Good Dog and Pocket Library switch row layouts at accessibility text sizes. Weather uses fixed-width columns (64, 40, 44 points) and a fixed 19-point toolbar font; Soundscape and Device Lab use fixed font sizes and raw colours. Only Weather reads Reduce Motion. No app has been checked with VoiceOver on the phone as a whole journey. *Source verified; nothing user validated.*
8. **Device Lab changes what we see.** Every app runs in a swipe-to-dismiss page sheet without its status bar or safe areas (`device-lab/Sources/AppHost/ContentView.swift:24`, friction audit D1, D3). Screenshots and phone sittings in Device Lab don't show the standalone layout, and a downward stroke from the top closed Sketchbook on the phone (2026-11-16). Every other finding's validation depends on fixing this first. *User validated (D1).*

What works and should be kept: native lists, forms and navigation everywhere; ForgeAppKit alerts and confirmations with consistent wording; `ForgeStateView` empty and failed states with an action; swipe actions mirrored in context menus (Chat, Commonplace, Good Dog, Pocket Library); accessibility identifiers on nearly every control; plain, kind copy (Good Dog's "Rest is part of learning. A day off never undoes your week." is the house voice at its best).

## How this audit was done

- Every app's SwiftUI screens were read on main, journey by journey, with counts across apps for the patterns in the [appendix](#appendix-pattern-counts).
- Existing screenshots were used where they exist: Pocket Library (2026-11-07, simulator), Weather (native redesign), the starter. They are review aids, not phone evidence.
- The [friction audit](https://claude.ai/artifact/HYycziassRTwYkXkwRCF4C) (2026-11-16) covered small functional and visual defects, some confirmed on the phone, and most are fixed or queued in the friction-fix PRs. Its rows are referenced as F1 to F23, V1 to V10 and D1 to D4, and not repeated.
- Soundscape is mid-change into a music player (#269 open) and Relay's Send and Receive screens are in #268. Both are audited on main, with a note where the open PR changes the answer.
- No simulator exists in this Linux setup and no phone was used for this pass. A [short phone sitting](#proposed-phone-sitting-about-10-minutes) would settle the visual questions source can't.

### Evidence levels

Every finding carries one:

| Level | Meaning |
|---|---|
| **S** Source verified | Confirmed by reading the implementation: the code does this |
| **V** Visually verified | Confirmed in a screenshot or by looking at the phone |
| **U** User validated | Confirmed by performing the workflow on the phone |

A finding at S says what the app does, not how it feels. Statements about understanding, speed or satisfaction (the three-question verdicts, and findings marked *judgement*) are hypotheses at S; they need U before they rank work on their own. Paths below are under `examples/<app>/Sources/AppHost/` unless they say otherwise; P1 findings name the file and symbol.

**Priority:** P1 hurts the first minute or the likely most common task; P2 hurts an experienced user or the feeling of quality; P3 is polish.
**Where:** *App* (that app only), *ForgeUI* (look: tokens, styles, components; no Rust, no AppKit), *ForgeAppKit* (behaviour patterns: state, confirmation, undo, search), *Check* (an automatic rule in `scripts/check-app-parity.sh` or a test), *Docs* (guidance in ios-ui.md).

## Cross-app findings

### 01 Visual design

| # | Finding | Apps | Ev. | Priority | Where |
|---|---|---|---|---|---|
| X1 | No configured custom icon in any app; iOS shows its default icon on the home screen and in Settings. `forge.toml` supports one 1024 px PNG today ([app-config.md](../../app-config.md)); `icon` is commented out in `commonplace`, `good-dog`, `relay`, `sketchbook`, `soundscape` `forge.toml` and absent elsewhere | All | S | P1 | App, plus a starter placeholder |
| X2 | One accent for every app: `.forgeTheme(.standard)` at the root (`ContentView.body` in Commonplace, FieldKit, Good Dog, Pocket Library, Relay, Sketchbook, Soundscape) or no theme (Chat, Checklist, Weather, Device Lab). ForgeUI already supports a per-app accent (`ForgePalette.withAccent`) with a contrast check; no app uses it | All | S | P1 | App (choose), ForgeUI (already capable) |
| X3 | No type roles beyond system text styles. System styles are right for Dynamic Type, but apps pick different styles for the same role (row title, metadata, section note), so hierarchy varies screen to screen | All | S | P2 | ForgeUI (named roles over system styles, not new fonts) |
| X4 | Raw colours for meaning: `.orange`, `.red`, `.green`, `.indigo` text and tints (Device Lab 13, Soundscape 13, Chat 8, Relay 8, FieldKit 6 occurrences). Some fail contrast (V2, V3; `orange-text` patch queued) | Most | S (V for V2) | P2 | Check (rule `toned-text`, queued), ForgeUI tones |
| X5 | Fixed font sizes outside large display numbers: Weather's bottom bar (19 pt), Soundscape's controls (34, 64 pt), Good Dog's celebration (96 pt), Device Lab's compass (44 pt). Display numbers are fine with `@ScaledMetric` (Weather does this); icons and controls should scale | Weather, Soundscape, Good Dog, Device Lab | S | P2 | Check (`.system(size:` outside `@ScaledMetric`), App |
| X6 | Little motion: list changes animate by default, but transitions between states (loading to content, a completed activity) mostly snap. Weather is the exception | Most | S | P3 | Docs, App |

### 02 Navigation

| # | Finding | Apps | Ev. | Priority | Where |
|---|---|---|---|---|---|
| X7 | Settings placement varies: a tab (Chat, Pocket Library), a gear sheet (Commonplace), a sheet inside a tab (Soundscape Listen), none elsewhere. Each can be right; the guidance should be "follow the app's information architecture, and say why", with a gear in the root toolbar as the usual starting point when settings are visited rarely | Chat, Pocket Library, Commonplace, Soundscape | S | P2 | Docs (guideline), App |
| X8 | Tab bar items have no accessibility identifiers in Chat (Pocket Library and Good Dog added theirs after the [ax spike](../ax-audit.md)) | Chat | S | P3 | App, Check |
| X9 | Inside Device Lab every app is a swipe-to-dismiss sheet, so "back" competes with "close the app" (D1, D3) | All, in Device Lab | U (D1) | P1 | Device Lab (full-screen cover with a visible close) |

### 03 Interaction

| # | Finding | Apps | Ev. | Priority | Where |
|---|---|---|---|---|---|
| X10 | No undo after a frequent, cheap destructive action. Checklist deletes on swipe with nothing to take it back; Commonplace's Mark All as Read is now confirmed (#274), and a confirmation for a routine action is friction of its own | Checklist, Commonplace | S | P2 | ForgeAppKit (an undo value the model owns, shown briefly; two apps need it) |
| X11 | Swipe actions not mirrored in a context menu or visible control are undiscoverable and harder for VoiceOver users. Mirrored: Chat, Commonplace feeds, Good Dog. Not: Checklist rename/delete, FieldKit trip delete, Relay forget | Checklist, FieldKit, Relay | S | P2 | Docs, Check (a `swipeActions` row also has `contextMenu`) |
| X12 | Rename is an alert with a text field in four apps (Commonplace, Sketchbook, Relay, FieldKit) and a sheet in Checklist. Both are native; the alert can't show a validation message, so an invalid name fails after it closes | Five apps | S | P3 | ForgeAppKit (one rename pattern that validates in place) |
| X13 | Sliders save only when the drag ends (F21): VoiceOver adjustments may be lost | Soundscape, Sketchbook | S | P2 | App, Check (queued idea) |

### 04 Feedback

| # | Finding | Apps | Ev. | Priority | Where |
|---|---|---|---|---|---|
| X14 | Success is mostly silent. Haptics: Good Dog 7, Sketchbook 3, Soundscape 3, Chat 2, Pocket Library 2, FieldKit 1, Relay 1, the starter 1 (19 in seven apps, 20 in eight projects); Checklist, Commonplace, Weather and Device Lab have none. Feedback should be proportional: a ticked checklist item or a finished session deserves a small response, a toggle doesn't | Most | S | P2 | Docs (which moments, proportionally), ForgeAppKit (four apps already trigger on a `saveCount` by hand: name the pattern) |
| X15 | Root loading is a bare `ProgressView()` with no label in Chat, Good Dog and Pocket Library. Usually instant; on a slow start VoiceOver hears nothing | Chat, Good Dog, Pocket Library | S | P3 | ForgeUI (`ForgeStateView.loading` exists; use it) |
| X16 | Search shows "No Results" while a search is still running (V6). Chat and Commonplace now track `resultsFor`; Checklist and Pocket Library each do it differently | Chat, Commonplace, Checklist, Pocket Library | S | P2 | ForgeAppKit (a search state; four apps) |
| X17 | Offline and server-unreachable states are worded and placed differently: a list row (Chat), the bottom toolbar (Commonplace, V4), a notice card (Weather), an alert (others) | Chat, Commonplace, Weather, Relay, Soundscape Listen | S | P2 | ForgeUI (one inline notice; Weather's `Notice` is the model), ForgeAppKit (`ErrorKind` drives retry: better-by-default item 6) |
| X18 | Permission denials: FieldKit, Weather and Chat's microphone say what is off and offer Settings. Relay's scanner silently disappears when the camera is denied (`PairingViews.swift`, `ScanCodeSheet.canScan` false), leaving a paste field with a `forgerelay://pair?…` placeholder | Relay | S | P2 | App; ForgeNative already has `ForgePermissions.openSettings` |

### 05 Accessibility and ergonomics

| # | Finding | Apps | Ev. | Priority | Where |
|---|---|---|---|---|---|
| X19 | Accessibility text sizes: Good Dog and Pocket Library switch row layout at AX sizes; Weather's 10-day rows use fixed widths that will likely clip; FieldKit's trip row (V5) wraps unevenly; the rest are unchecked | Weather, FieldKit, others | S | P2 | App; a baseline at `-ax5` per app (visual regression supports variants) |
| X20 | Reduce Motion is read only by Weather. Little else animates, so the risk is low today; it grows with phase 3 | All | S | P3 | Docs |
| X21 | One-handed reach. Toolbar actions (`+`, New Sketch, Add Feed) sit at the top right in every app, which is the iOS convention and fine for occasional actions. An app's main *repeated* action is better at the bottom: Chat's composer and Checklist's add field already are; FieldKit's Record is a top-right toolbar button | FieldKit | S | P2 | App |
| X22 | No VoiceOver journey has been run on the phone for any app; identifiers and labels are present, the experience is unproven | All | S | P1 for phase 5 | Phase 5 sitting |
| X23 | Keyboard: only Chat has a keyboard shortcut. Matters only if iPad comes back | All | S | P3 | Record |

### 06 Product experience and coherence

| # | Finding | Apps | Ev. | Priority | Where |
|---|---|---|---|---|---|
| X24 | First run asks for infrastructure: Chat (`SignUpView`: server URL field and a `make chat-server` footer), Soundscape Listen (`ListenModel.Phase.needsServer` until a take server is saved), Relay (`DevicesView.emptyState`: pairing before anything else). Honest for a demo, but likely blocks "understand it without being taught" | Chat, Soundscape, Relay | S (confusion: judgement) | P1 | App (options per app below) |
| X25 | Empty apps teach little. Commonplace, FieldKit, Sketchbook, Checklist and Pocket Library start empty with a good one-line explanation and one action. Optional sample content or a guided first step could help where it fits (suggested feeds in Commonplace); not where the data is personal (Checklist, FieldKit), where an empty start is correct | Five apps | S | P2 | App, case by case |
| X26 | The apps share an ecosystem only through Device Lab. There is no written voice guide (Good Dog's tone is the best and is not written down), no notion of each app's accent, and no guidance for choosing between two native patterns (alert vs sheet for rename, tab vs sheet for settings) | All | S | P2 | Docs (phase 2 design direction) |

## App by app

Each app has: **what it's trying to be** (so phase 2 respects its personality), the **three questions** with a verdict (a judgement from source until phase 5), and its findings. Findings already covered above or in the friction audit are only referenced.

### Chat

**Trying to be:** a private, everyday messenger for a small group, with an on-device assistant on the side. It should feel immediate and trustworthy: you always know whether your message got there. Personality: quiet and dependable, closer to Signal than to a social app.

| Question | Verdict (judgement) |
|---|---|
| New user | **Likely not.** The first screen is "Welcome to Chat" with a name field and a server URL field whose footer says to run `make chat-server`. Nothing says what Chat is or who you'd talk to |
| Experienced user | **Mostly.** Reply, react, edit, delete, voice messages, photos, documents, search, mute and swipe to mark read are all there. Delivery states (Sending, Delivered, Read, Not Delivered, tap to try again) are clear |
| Polish | **Partly.** Send and failure haptics exist. No theme, no identity |

| # | Dimension | Finding | Reference | Ev. | Priority | Where |
|---|---|---|---|---|---|---|
| C1 | Product | First run is a server form (`SignUpView.body`, `ChatServers.hosted` is nil). A hosted default would remove the friction but raises privacy, security, ownership, availability and cost questions for a private messenger. **To investigate, not assumed:** (a) a local demo or sample environment that needs no server; (b) an optional hosted service with explicit consent and clear data handling; (c) an invitation or existing-server flow (join by link or code), with self-hosting as an advanced option. The aim is removing technical friction without changing what Chat is | Signal: nothing technical at first run | S | P1 | App (decision) |
| C2 | Product | Sign Out loses the account for good (F5, wording fixed). There is still no way back in | Signal: re-register, transfer | S | P2 | App + Rust |
| C3 | Visual | No `forgeTheme` in `ContentView` and no accent of its own; the flagship looks the most generic (X2) | | S | P1 | App |
| C4 | Interaction | Reactions are a fixed emoji list in a context menu | Signal, iMessage: a reaction bar on long press | S | P3 | App |
| C5 | Feedback | Delivery is text under the newest message only. Clear, but wordy in a busy room | Signal: compact delivery marks, detail on tap | S | P3 | App |
| C6 | Navigation | Assistant is a full tab, unrelated to conversations; without Apple Intelligence the tab disappears and the bar changes | | S | P3 | App |
| C7 | Accessibility | Tab items have no identifiers (X8) | | S | P3 | App |

### Good Dog

**Trying to be:** a kind, encouraging coach for training your dog in a few minutes a day. It should feel warm, calm and never judgmental. Personality: friendly and gentle; the copy carries it today, the visuals don't yet.

| Question | Verdict (judgement) |
|---|---|
| New user | **Yes.** The welcome asks about your dog and the time you have, then shows today's plan. The best first minute in ixOS |
| Experienced user | **Yes, with density.** Today is a long grouped list: progress, rhythm, goal, activities, then "Only a Few Minutes Today" and "Not Today" |
| Polish | **Mostly.** Haptics, a stage celebration, a guided session. Visually a standard grouped list in the standard blue, which undersells a warm app |

| # | Dimension | Finding | Reference | Ev. | Priority | Where |
|---|---|---|---|---|---|---|
| G1 | Visual | The warmest app wears the default theme. An accent and a few illustrative symbols could carry the personality the copy already has (X2) | Coaching apps: colour as encouragement | S | P2 | App |
| G2 | Visual | Two equally prominent full-width buttons on the exercise screen (V1) | | V (baseline, 2026-11-15) | P2 | App |
| G3 | Navigation | Today mixes status, goal, plan and day choices in one list; the plan is the third section | | S | P2 | App |
| G4 | Interaction | Guided session: the screen can lock mid-session (F18) and Close ends it with no confirmation (`GuidedSessionView.swift:55`, F19). Hands are busy with the dog | | S | P1 | App (keep-screen-on has a second use in Pocket Library, so the helper can be extracted) |
| G5 | Feedback | Finishing the day's plan gets "All done for today" text; the celebration is kept for stages | | S | P3 | App |

### FieldKit

**Trying to be:** a field notebook: press record, walk, note things, and come home to a map and the numbers. It should feel rugged and glanceable, usable with one hand outdoors. Personality: practical, map-first.

| Question | Verdict (judgement) |
|---|---|
| New user | **Yes.** "No Trips" explains it and offers Record a Trip |
| Experienced user | **Mostly.** Starting a trip is Record, a sheet with a pre-filled name, then Start: one tap more than needed. Background recording is on (#256) |
| Polish | **Partly.** The live map and stats are clear. Raw orange warnings (V3); trip rows wrap (V5) |

| # | Dimension | Finding | Reference | Ev. | Priority | Where |
|---|---|---|---|---|---|---|
| K1 | Interaction | Record opens a naming sheet before anything records (`TripListView.swift:55`, `StartTripView`). Starting with the default name and renaming later (rename exists) saves a step | Workout and hiking apps: one big Start | S | P1 | App |
| K2 | Ergonomics | Record is a top-right toolbar button; during a trip Stop and Add Note are at the bottom (X21) | | S | P2 | App |
| K3 | Visual | Trip rows have no map thumbnail; every trip looks the same | | S | P3 | App |
| K4 | Interaction | Trip delete is swipe-only (X11) | | S | P3 | App |
| K5 | Feedback | No pause; a coffee stop counts as moving time | | S | P2 | App + Rust |

### Soundscape

**Trying to be (from #269 on):** a music player for Reuben's own generated songs, with ways to compare takes and pick favourites. It should feel like a good music app: artwork, a queue, Now Playing, lock-screen controls. Personality: warm, immersive, audio-first.

Two different states, kept apart here:
- **Listen tab, on main: user validated.** Songs, takes, A/B at matched loudness, stars, notes, picks and lock-screen playback passed on the phone in Device Lab (2026-11-16, README L1).
- **The music library (#269 and the PRs after it): not audited.** It is unfinished and unmerged; this audit doesn't judge it. Main still carries the Studio (projects, tracks, mixer), which that series removes, so Studio findings are left out.

| Question | Verdict (judgement) |
|---|---|
| New user | **Not yet.** Listen needs a take server address the first time; the first tab is the Studio. After the #269 series the library is the first screen |
| Experienced user | **Yes for Listen** (user validated, above) |
| Polish | **Partly.** Generated gradient artwork, mini player and Now Playing. Fixed font sizes (X5), raw orange and red (X4), and no interruption handling (F8, fix queued) |

| # | Dimension | Finding | Reference | Ev. | Priority | Where |
|---|---|---|---|---|---|---|
| S1 | Product | First use asks for the take server (`ListenModel.Phase.needsServer`); it is remembered after that. As a personal app it could prefill the laptop's address | Apple Music: content first | S | P2 | App (in the #269 series) |
| S2 | Visual | Transport icons at a fixed 64 pt (`ListenView.swift`, Now Playing) won't follow Dynamic Type or Bold Text | Apple Music | S | P2 | App |
| S3 | Feedback | Audio interruptions leave the transport wrong (F8: `LoopEngine` has no interruption observers; recording failed and Device Lab crashed once on the phone) | | U (F8) | P1 | App (queued: `soundscape-fixes`) |
| S4 | Interaction | Stars at caption size in orange; small targets for a frequent action | | S | P2 | App |

### Relay

**Trying to be:** "Your devices. One network." Moving a file to your own other device, or to someone nearby, should be as simple as AirDrop and feel safe. Personality: trustworthy and quick; security is felt, not explained.

| Question | Verdict (judgement) |
|---|---|
| New user | **Partly.** The empty state explains pairing and offers Show My Code. Sending without pairing (word codes) is hidden in a `+` menu labelled Pair a Device (V7); #268 adds Send and Receive screens, which should answer this |
| Experienced user | **Partly.** A paired device is one tap away. Both devices must have Relay open on the same Wi-Fi |
| Polish | **Partly.** Success haptic, Quick Look preview. A bare pencil renames this device (V7) |

| # | Dimension | Finding | Reference | Ev. | Priority | Where |
|---|---|---|---|---|---|---|
| R1 | Navigation | The app is organised around devices (`ContentView.swift`, `DevicesView`: paired list, `+` menu "Pair a Device" at line 118); the task is "send this" or "receive". Lead with Send and Receive, devices as targets | LocalSend: Receive, Send, Settings | S | P1 | App (check #268 first) |
| R2 | Feedback | Failed rows show Rust diagnostics (F10); large-file sends froze (F9, fixed by #282's `ForgePickedFile`) | LocalSend: per-file progress, accept/decline on the receiver | S | P2 | App |
| R3 | Feedback | "Open while Relay is on screen" is the real constraint and is in a list footer; people will send and lock the phone | | S | P2 | App |
| R4 | Interaction | Forget is swipe-only (X11); camera denial is silent (X18) | | S | P3 | App |

### Sketchbook

**Trying to be:** a pocket drawing notebook: open, draw, done. It should feel tactile and calm, with the canvas as the whole point. Personality: paper and ink.

| Question | Verdict (judgement) |
|---|---|
| New user | **Yes.** "No Sketches" with New Sketch; paper choice is a long press on `+`, so the default is one tap |
| Experienced user | **Mostly.** PencilKit tools, named undo and redo, layers, export with preview |
| Polish | **Mostly.** Paper backgrounds give it character. The canvas sits under a navigation bar; no focus mode |

| # | Dimension | Finding | Reference | Ev. | Priority | Where |
|---|---|---|---|---|---|---|
| SK1 | Navigation | No full-screen drawing; the navigation bar and tool picker take a large share of a phone screen | Notes, Freeform: hide chrome while drawing | S | P2 | App |
| SK2 | Interaction | In Device Lab a downward stroke from the top starts closing the app (D1) | | U (D1) | P1 | Device Lab (`ContentView.swift:24`, `.sheet(item: $openApp)`) |
| SK3 | Visual | Paper colours are fixed RGB with no dark variant; a light page in dark mode is a reasonable choice worth stating | Paper apps keep the page light | S | P3 | App |

### Weather

**Trying to be:** glanceable weather for here and the places you care about. It should feel atmospheric and instant. Personality: the most designed app in ixOS, a drawn sky per condition.

| Question | Verdict (judgement) |
|---|---|
| New user | **Yes.** It shows My Location's weather straight away, or a clear empty page with Use My Location |
| Experienced user | **Yes.** Swipe between places, scroll for the 10-day and details |
| Polish | **Yes, visually.** But its layout is very close to Apple's Weather, so its strongest look is not its own |

| # | Dimension | Finding | Reference | Ev. | Priority | Where |
|---|---|---|---|---|---|---|
| W1 | Visual | The layout follows Apple Weather closely (big temperature, hourly strip, 10-day bars, tiles, page dots). Phase 2 should decide Weather's own idea rather than polish a likeness | Open-source weather apps ([references.md](references.md)) | V (`weather-native/3-sunny-light.jpg`) | P2 | App (decision) |
| W2 | Accessibility | Fixed column widths in day rows (`ForecastPage.swift`, `DayRow`) and a fixed 19 pt bottom bar (`WeatherView.swift`, `BottomBar`) | | S | P2 | App |
| W3 | Feedback | Location staleness and add-city errors: F1 fixed (#255), F14/F15 in #283 | | U (F1) | done | |
| W4 | Product | No weather warnings | Apple Weather, national services | S | P3 | App + Rust (if the data source has them) |

### Commonplace

**Trying to be:** a calm, local-first reading place for the web you choose: feeds now, a full personal reading system later (decided 2026-11-16). It should feel quiet and focused. Personality: bookish and unhurried.

| Question | Verdict (judgement) |
|---|---|
| New user | **Partly.** "No Feeds Yet" explains feeds and offers Add Feed, but needs a feed URL; a site address fails (F11) |
| Experienced user | **Partly.** Smart lists, unread counts, swipe read and bookmark, search, reader text settings. No next article in the reader |
| Polish | **Mostly.** Toned smart-list rows; the refresh status may truncate (V4) |

| # | Dimension | Finding | Reference | Ev. | Priority | Where |
|---|---|---|---|---|---|---|
| CP1 | Interaction | No next article or next unread in the article screen (`ArticleView.swift`, `toolbar`: text settings, bookmark, read state, share; no navigation between articles) | NetNewsWire: next unread, move between articles | S | P1 | App |
| CP2 | Product | Feed discovery from a site address (F11; `AddFeedView.swift:25` asks for a feed address). Optional suggested feeds on first run | NetNewsWire: discovery | S | P1 | App + Rust |
| CP3 | Interaction | Mark All as Read is confirmed (#274); an undo would be lighter (X10) | NetNewsWire | S | P3 | ForgeAppKit (X10) |
| CP4 | Visual | Article rows are dense text; no feed icons | NetNewsWire: favicons | S | P3 | App |

### Checklist

**Trying to be:** the quickest list on the phone: type, tick, done. It is growing into due dates, recurring tasks and reminders (decided 2026-11-16). Personality: crisp and fast.

| Question | Verdict (judgement) |
|---|---|
| New user | **Yes.** An add field at the top and "Nothing to do" |
| Experienced user | **Partly.** Adding keeps focus for the next item, which is right. Done items stay mixed with open ones; no reordering; rename and delete are swipe-only |
| Polish | **No.** No ForgeUI, no theme, no haptics. Share opens a half sheet before the system share sheet (V8) |

| # | Dimension | Finding | Reference | Ev. | Priority | Where |
|---|---|---|---|---|---|---|
| L1 | Interaction | Swipe delete with no confirmation and no undo (X10) | Reminders, Things | S | P2 | ForgeAppKit (X10) |
| L2 | Interaction | Done items mixed with open ones; no Completed section or hide-done | Reminders: Show/Hide Completed | S | P2 | App |
| L3 | Feedback | Ticking an item has no haptic or animation; it's the app's most frequent action | Things, Reminders | S | P2 | App |
| L4 | Visual | Not on ForgeUI or a theme | | S | P2 | App |
| L5 | Interaction | Share sheet in front of the share sheet (V8) | | S | P3 | App |

### Pocket Library

**Trying to be:** a personal reading life: what you're reading now, what's next, and a good place to read and listen. It is also ixOS's reference app. Personality: cosy and literate; covers carry it.

| Question | Verdict (judgement) |
|---|---|
| New user | **Yes.** An empty library with an Add menu (catalogue, manual, EPUB) |
| Experienced user | **Partly.** Continue Reading is three levels down; Reading Now is under Activity |
| Polish | **Mostly.** Generated cover placeholders, read-aloud with lock-screen controls, summaries, AX layouts. Stars now toned (V2) |

| # | Dimension | Finding | Reference | Ev. | Priority | Where |
|---|---|---|---|---|---|---|
| PL1 | Navigation | Continuing a book: Library, the book, Continue Reading (`BookDetailView.swift:175`); "Reading Now" is in the Activity tab (`ActivityView.swift:18`). The current book(s) at the top of Library with a one-tap Continue would serve the likely most common task | Apple Books: Reading Now first | S (V: `2-library-light.jpg`) | P1 | App |
| PL2 | Navigation | Settings is a full tab (X7) | | S | P3 | App |
| PL3 | Interaction | Catalogue: only the small `+` adds a book (F20) | | S | P2 | App |
| PL4 | Product | As the reference app it should adopt phase 3 patterns first; its README screenshot placeholder is empty | | S | P2 | App |

### Device Lab

**Trying to be:** two things. For Reuben, the one app on the phone that hosts every ixOS app for testing. For a developer, a live readout of what an iPhone tells an app. Personality: an instrument panel: dense, precise, honest about permissions.

| Question | Verdict (judgement) |
|---|---|
| New user | **Partly.** The dashboard is clear as an instrument panel; the hosted apps are behind a toolbar icon |
| Experienced user | **Partly.** Open at Launch is a good shortcut. Opening another app means swipe down, Apps, pick |
| Polish | **Mostly.** Dense and well labelled; raw colours (13) and a fixed compass font |

| # | Dimension | Finding | Reference | Ev. | Priority | Where |
|---|---|---|---|---|---|---|
| D-1 | Navigation | Apps is a toolbar `NavigationLink` (`DashboardView.swift:59`, `AppsView`). Make the app list the root, or a tab beside the dashboard, since hosting apps is what the phone uses it for now | Apple's developer tools: task first, detail on demand | S | P1 | Device Lab |
| D-2 | Navigation | Full-screen app presentation with a visible way back (`ContentView.swift:24`; X9, D1, D3) | | U (D1) | P1, first | Device Lab |
| D-3 | Product | "This Build" is the only proof of which build is installed; keep it one tap from anywhere | | S | P3 | Device Lab |

### The starter (`forge init`)

**Trying to be:** the example every new app copies. Whatever it does, the next app does.

It uses ForgeUI, `.forgeTheme(.standard)`, `ForgeStateView` and ForgeAppKit, so new apps start better than the old ones. Two gaps carry straight into every new app: no placeholder icon (X1), and the standard accent with no prompt to choose one (X2). The larger aim is a starter that begins with a good experience, not just a working core: when phase 3 adds patterns, the starter adopts them in the same PR (rule 12, app parity).

## Shared gap analysis: what goes where

The package strategy decides each piece's home. ForgeUI is the standalone SwiftUI design system (presentation; no Rust, no AppKit); ForgeAppKit is the optional layer of behaviour patterns on top of it.

| Need | Home | Why there | Second use |
|---|---|---|---|
| Per-app accent and identity | ForgeUI (exists: `withAccent`) + each app | Already possible; nobody uses it | All apps |
| Named text roles (row title, metadata, section note) over system styles | ForgeUI | Pure presentation, useful to any SwiftUI app | All apps vary |
| Inline notice (offline, stale, permission) | ForgeUI | A component; Weather's `Notice` and Chat's banner are two uses | Weather, Chat, Commonplace |
| Undo after a destructive action | ForgeAppKit | A model value with an action and a timeout, like `ForgeAlert` | Checklist, Commonplace |
| Search state (debounce, in flight, no results) | ForgeAppKit | Behaviour, not look | Chat, Commonplace, Checklist, Pocket Library |
| Rename with validation | ForgeAppKit | Five apps, two shapes, none validates in place | Commonplace, Sketchbook, Relay, FieldKit, Checklist |
| Keep the screen on during a task | ForgeAppKit (or a ForgeKit service) | Second use | Pocket Library, Good Dog |
| Success feedback on a model counter | ForgeAppKit | Four apps write a `saveCount` trigger by hand | FieldKit, Relay, Sketchbook, Good Dog |
| Settings placement, rename, confirm vs undo, proportional feedback | Docs (ios-ui.md) | Guidance, not code | All |
| Toned text, fixed font sizes, swipe without menu, tab identifiers | Check (parity rules) | Cheap to grep, keep coming back | X4, X5, X8, X11 |
| Accessibility tree in baselines | forge-uitest (`.tree.json` is saved with every screenshot) | Diff it like the pixels; needs your go-ahead, `forge ui` is frozen | Pocket Library baselines |

Not proposed: a button, card or list library beyond what ForgeUI has; a navigation framework; custom fonts; one look for every app.

## Proposed phase 2 direction

A short design-direction doc, not a redesign. The aim behind it is an ixOS product-design philosophy: not one appearance, but a recognisable standard for how an ixOS app welcomes someone, explains itself, handles mistakes, communicates success and makes its most important task easy, starting with what `forge init` produces.

1. **Identity kit per app:** an icon, an accent (checked by `contrastIssues()`), a symbol set, and one line of personality from this audit.
2. **Interaction guidelines, not rules.** Five starting principles, each with its usual native pattern and the known exceptions:
   - *Make the main task easy to reach.* Usually close to launch; but "main" is a judgement to validate, and some apps have several equal tasks.
   - *Undo frequent, cheap actions; confirm rare, costly ones.* Some actions can't be undone (a sent message, a remote delete) and keep a confirmation.
   - *Feedback in proportion to the action.* A finished session or a sent file deserves a response; a toggle doesn't. Never leave someone unsure whether something worked.
   - *First run works without infrastructure, where that's possible.* Sample content only where it helps and never mixed into personal data; an empty start is right for a personal checklist or trip log.
   - *Give swipe actions a visible twin* (context menu or button), for discoverability and VoiceOver.
   Settings placement follows each app's information architecture, with a reason written down.
3. **A small foundations list** for phase 3: text roles and the inline notice (ForgeUI); undo, search state and success feedback (ForgeAppKit); the parity checks above. Pocket Library adopts first, the starter in the same PR.
4. **Redesign order is not decided here.** The one firm proposal is Device Lab's full-screen hosting first, because it is needed to evaluate every other app. After that, the reference audits and Reuben's priorities set the order; this audit's P1 list is an input, not the plan.

## Decisions for Reuben

1. **Where shared pieces live.** Look in ForgeUI, behaviour in ForgeAppKit, as the package strategy says. *Agreed in review (2026-11-17).*
2. **App icons and accents:** who makes the icons (you, a designer, or generated placeholders for now)? Names are provisional, so icons can be too. Recommendation: simple placeholders now, real ones before any App Store release.
3. **Undo vs confirmation** as an ixOS guideline. Recommendation: yes, as a guideline with exceptions.
4. **Weather's identity:** refine the Apple-like layout, or give it its own idea? Recommendation: its own idea, decided in phase 2.
5. **Reference for Pocket Library.** NetNewsWire for Commonplace (its benchmark since #148) and the reading view; Apple Books (behaviour only) for Pocket Library. See [references.md](references.md).
6. **Chat's first run:** which of the three options in C1 to investigate first. Recommendation: (c) invitation or existing server, then (a) a local demo; (b) hosted only with a clear data-handling story.
7. **Device Lab full-screen hosting** first in implementation. *Emphasised in review (2026-11-17).*

## Proposed phone sitting (about 10 minutes)

Not blocking; it moves findings from S to V or U. On the Device Lab build already installed, nothing to build first.

1. **Liquid Glass, 1 min.** Open Pocket Library: is the tab bar the iOS 26 glass bar or the older opaque one? Linux xtool builds may run in compatibility mode ([forge-ui-testing.md](../../forge-ui-testing.md)). This decides what "native" looks like for the whole design direction.
2. **Largest text, 3 min.** Settings, Accessibility, Larger Text, the largest size. Open Weather (10-day rows), FieldKit (trip list), Checklist and Chat. Screenshot anything clipped (X19, W2).
3. **VoiceOver, 4 min.** VoiceOver on, Checklist: add an item, tick it, delete it. Then Pocket Library: continue a book (X22, PL1).
4. **Home screen, 30 s.** A screenshot of Device Lab's icon, the "before" of X1.

## Appendix: pattern counts

Counts of source lines across each app's Swift (generated bindings excluded) on `15bc21d`. Used to spot patterns, not as scores.

| Pattern | chat | check | common | devlab | field | gooddog | pocket | relay | sketch | sound | weather | starter |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `forgeTheme(` | 0 | 0 | 1 | 0 | 1 | 1 | 1 | 1 | 1 | 1 | 0 | 1 |
| `ForgeStateView` | 0 | 0 | 4 | 0 | 1 | 4 | 2 | 1 | 1 | 8 | 0 | 2 |
| `ContentUnavailableView` | 5 | 2 | 1 | 1 | 0 | 3 | 10 | 2 | 1 | 1 | 1 | 1 |
| `.forgeAlert` | 4 | 0 | 4 | 0 | 4 | 8 | 7 | 2 | 3 | 4 | 0 | 4 |
| `.system(size:` | 0 | 0 | 0 | 1 | 0 | 1 | 0 | 0 | 0 | 4 | 5 | 0 |
| Raw colour names | 8 | 0 | 1 | 13 | 6 | 0 | 6 | 8 | 1 | 13 | 0 | 0 |
| `accessibilityIdentifier` | 37 | 19 | 36 | 18 | 31 | 87 | 77 | 46 | 35 | 90 | 13 | 17 |
| `accessibilityLabel` | 8 | 2 | 3 | 1 | 6 | 19 | 15 | 4 | 6 | 28 | 6 | 1 |
| Haptics | 2 | 0 | 0 | 0 | 1 | 7 | 2 | 1 | 3 | 3 | 0 | 1 |
| Animation | 7 | 0 | 0 | 1 | 0 | 2 | 2 | 0 | 0 | 0 | 3 | 0 |
| `swipeActions` | 4 | 1 | 3 | 1 | 2 | 1 | 2 | 1 | 1 | 6 | 0 | 1 |
| `contextMenu` | 3 | 0 | 1 | 0 | 0 | 1 | 1 | 1 | 2 | 3 | 0 | 1 |
| Reads Reduce Motion | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 | 0 |
| Custom icon configured | no | no | no | no | no | no | no | no | no | no | no | no |

Haptics total 19 in the seven apps that have any, 20 with the starter.
