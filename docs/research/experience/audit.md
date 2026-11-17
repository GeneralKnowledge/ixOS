# ixOS experience audit, phase 1

Status: 2026-11-17, on main at `15bc21d` (after #282). Part of [ixOS Experience Research](README.md). Research only: no app, ForgeUI or ForgeAppKit change is made here.

## The short version

The apps are better built than they are designed. Their states, errors and accessibility basics are mostly right, because the framework and the parity checks made them right. What is missing is the layer above: deliberate identity, a clear first minute, and the fastest path for the task people do most.

The eight findings that matter most:

1. **Nothing gives an app its own identity.** Our own rule says identity comes from the accent colour, the app icon, symbols and copy ([ios-ui.md](../../ios-ui.md)). No app has an icon (every `forge.toml` has `icon` commented out or absent), and every app that sets a theme uses `.forgeTheme(.standard)`, the same blue. Chat, Checklist, Weather and Device Lab set no theme at all. Ten apps look like one app with different lists.
2. **The first minute is the weakest part of most apps.** Chat asks a new user for a server address and a `make chat-server` command. Soundscape's Listen asks for a take server. Commonplace starts empty and needs a feed URL; a site address fails. Relay starts with "No Devices" and a QR code. Good Dog is the exception: it asks about your dog and gives you today's plan.
3. **The most common task is often not the most prominent.** Pocket Library's "Reading Now" lives in the Activity tab, so continuing a book is Library, find it, open it, Continue Reading. Commonplace's article screen has no next article, so reading a timeline means back, next row, back. Device Lab's Apps, the reason it is on the phone, is a toolbar icon above a long sensor dashboard.
4. **Recovering from a mistake means a confirmation, never an undo.** Deletes are either confirmed (Pocket Library, FieldKit, Sketchbook, Chat) or immediate and permanent (Checklist's swipe to delete). No app offers undo after the fact, outside Sketchbook's canvas. iOS's own answer for frequent, cheap actions is undo or Recently Deleted; for rare, costly ones it is a confirmation. We use one answer for both.
5. **Settings live in a different place in each app.** A tab in Chat and Pocket Library, a sheet from a gear in Commonplace, a sheet inside Listen in Soundscape, nowhere in the rest. Primary actions are mostly a `+` at the top right, which is right; FieldKit's Record, the app's one big action, is a small toolbar icon.
6. **Feedback is good in the states and thin in the moments.** Empty, loading and failed states exist almost everywhere (`ForgeStateView` or `ContentUnavailableView`). What is thin is the moment of success: haptics appear in 19 places across ten apps, animation in a handful, and most saves happen silently. An app should never leave someone wondering whether something worked; ours mostly don't, but they rarely feel satisfying either.
7. **Accessibility is strong where an app was reviewed, unproven elsewhere.** Good Dog and Pocket Library switch row layouts at accessibility text sizes and label combined elements carefully. Weather uses fixed-width columns (64, 40, 44 points) and a fixed 19-point toolbar font; Soundscape and Device Lab use fixed font sizes and raw colours. Only Weather reads Reduce Motion. No app has been checked with VoiceOver on the phone as a whole journey.
8. **Device Lab changes what we see.** Every app runs in a swipe-to-dismiss page sheet without its status bar or safe areas (friction audit D1, D3). Screenshots and phone sittings in Device Lab don't show the standalone layout, and a downward stroke or pull can close the app. Experience validation (phase 5) needs a full-screen presentation first.

What works and should be kept: native lists, forms and navigation everywhere; ForgeAppKit alerts and confirmations with consistent wording; `ForgeStateView` empty and failed states with an action; swipe actions mirrored in context menus (Chat, Commonplace, Good Dog, Pocket Library); accessibility identifiers on nearly every control; plain, kind copy (Good Dog's "Rest is part of learning. A day off never undoes your week." is the house voice at its best).

## How this audit was done

- Every app's SwiftUI screens were read on main, journey by journey, with counts across apps for the patterns in the [appendix](#appendix-pattern-counts).
- Existing screenshots were used where they exist: Pocket Library (2026-11-07, simulator), Weather (native redesign), the starter. They are review aids, not phone evidence.
- The [friction audit](https://claude.ai/artifact/HYycziassRTwYkXkwRCF4C) (2026-11-16) covered small functional and visual defects, and most are fixed or queued in the friction-fix PRs. Its rows are referenced as F1 to F23 and V1 to V10 and not repeated.
- Soundscape is mid-change into a music player (#269 open) and Relay's Send and Receive screens are in #268. Both are audited on main, with a note where the open PR changes the answer.
- Evidence level is **source** unless a row says otherwise. No simulator exists in this Linux setup and no phone was used. A [short phone sitting](#proposed-phone-sitting-about-10-minutes) would settle the visual questions source can't.

**Priority:** P1 hurts the first minute or the most common task; P2 hurts an experienced user or the feeling of quality; P3 is polish.
**Where:** *App* (that app only), *ForgeUI* (look: tokens, styles, components; no Rust, no AppKit), *ForgeAppKit* (behaviour patterns: state, confirmation, undo, search), *Check* (an automatic rule in `scripts/check-app-parity.sh` or a test), *Docs* (a rule in ios-ui.md).

## Cross-app findings

### 01 Visual design

| # | Finding | Apps | Priority | Where |
|---|---|---|---|---|
| X1 | No app icon in any app; every app shows iOS's blank icon on the home screen and in Settings. `forge.toml` supports one 1024 px PNG today ([app-config.md](../../app-config.md)) | All | P1 | App, plus a starter placeholder |
| X2 | One accent for every app (`.forgeTheme(.standard)`, or nothing). ForgeUI already supports a per-app accent (`ForgePalette.withAccent`) with a contrast check; no app uses it | All | P1 | App (choose), ForgeUI (already capable) |
| X3 | No type scale beyond system text styles. That is right for Dynamic Type, but every app picks its own mix of `.headline`, `.subheadline`, `.caption` for the same roles (row title, metadata, section note), so hierarchy varies screen to screen | All | P2 | ForgeUI (named roles over system styles, not new fonts) |
| X4 | Raw colours for meaning: `.orange`, `.red`, `.green`, `.indigo` text and tints (Device Lab 13, Soundscape 13, Chat 8, Relay 8, FieldKit 6 occurrences). Some fail contrast (V2, V3; `orange-text` patch queued) | Most | P2 | Check (rule `toned-text`, queued), ForgeUI tones |
| X5 | Fixed font sizes outside large display numbers: Weather's bottom bar (19 pt), Soundscape's controls (34, 64 pt), Good Dog's celebration (96 pt), Device Lab's compass (44 pt). Large display numbers are fine with `@ScaledMetric` (Weather does this); icons and controls should scale | Weather, Soundscape, Good Dog, Device Lab | P2 | Check (`.system(size:` outside `@ScaledMetric`), App |
| X6 | Motion is almost absent: list changes animate by default, but transitions between states (loading to content, a completed activity, a sent message) mostly snap. Weather is the exception | Most | P3 | Docs (when to animate), App |

### 02 Navigation

| # | Finding | Apps | Priority | Where |
|---|---|---|---|---|
| X7 | Settings has no home: a tab (Chat, Pocket Library), a gear sheet (Commonplace), a sheet inside a tab (Soundscape Listen), none elsewhere. A tab spends a third of the tab bar on something visited rarely | Chat, Pocket Library, Commonplace, Soundscape | P2 | Docs (one rule: a gear in the root screen's toolbar, unless the app has three or more real tabs), App |
| X8 | Tab bars have no identifiers in Chat (Pocket Library and Good Dog fixed theirs after the [ax spike](../ax-audit.md)) | Chat | P3 | App, Check (tab items need identifiers) |
| X9 | Inside Device Lab, every app is a swipe-to-dismiss sheet, so back navigation competes with "close the app" (friction audit D1, D3) | All, in Device Lab | P1 for testing | Device Lab (full-screen cover with a visible close) |

### 03 Interaction

| # | Finding | Apps | Priority | Where |
|---|---|---|---|---|
| X10 | No undo after a frequent, cheap destructive action. Checklist deletes on swipe with nothing to take it back; Commonplace's Mark All as Read is now confirmed (#274) but a confirmation for a routine action is friction of its own | Checklist, Commonplace | P2 | ForgeAppKit (an undo value the model owns, shown as a short banner; second use exists: Checklist and Commonplace) |
| X11 | Swipe actions are not always mirrored in a context menu or a visible control, so they are undiscoverable and hard for VoiceOver users. Good: Chat, Commonplace feeds, Good Dog. Missing: Checklist rename/delete, FieldKit trip delete, Relay forget | Checklist, FieldKit, Relay | P2 | Docs, Check (a `swipeActions` row also has `contextMenu`) |
| X12 | Rename is done in an alert with a text field in four apps (Commonplace, Sketchbook, Relay, FieldKit) and in a sheet in Checklist. Both are native; the alert has no validation message, so an invalid name fails after the alert closes | Five apps | P3 | ForgeAppKit (one rename pattern that validates in place) |
| X13 | Sliders save only when the drag ends (F21): VoiceOver adjustments may be lost, and Soundscape's mix can't be judged by ear while dragging | Soundscape, Sketchbook | P2 | App, Check (queued idea) |

### 04 Feedback

| # | Finding | Apps | Priority | Where |
|---|---|---|---|---|
| X14 | Success is mostly silent. Haptics exist in 19 places (Good Dog 7, the rest 0 to 3); Checklist, Commonplace and Weather have none. A completed checklist item, a saved trip, a sent file or a finished session should feel finished | Most | P2 | Docs (which moments get which feedback), ForgeAppKit (`saveCount`-style triggers already exist in four apps: name the pattern) |
| X15 | Root loading is a bare `ProgressView()` with no label in Chat, Good Dog and Pocket Library. Usually instant, but on a slow start VoiceOver hears nothing and the screen is blank | Chat, Good Dog, Pocket Library | P3 | ForgeUI (`ForgeStateView.loading` already exists; use it) |
| X16 | Search shows "No Results" while a search is still running (V6). Fixed per app with a `resultsFor` key in Chat and Commonplace; Checklist and Pocket Library each do it differently | Chat, Commonplace, Checklist, Pocket Library | P2 | ForgeAppKit (a search state; four apps now, well past the second use) |
| X17 | Offline and server-unreachable states are worded per app and placed differently: a list row (Chat), the bottom toolbar (Commonplace, V4), a notice card (Weather), an alert (others) | Chat, Commonplace, Weather, Relay, Soundscape Listen | P2 | ForgeUI (one inline notice component; Weather's `Notice` is the model), ForgeAppKit (`ErrorKind` drives retry: better-by-default item 6) |
| X18 | Permission denials: FieldKit, Weather and Chat's microphone say what is off and offer Settings. Relay's scanner silently disappears when the camera is denied (`DataScannerViewController.isAvailable` is false), leaving a paste field with a `forgerelay://pair?…` placeholder | Relay | P2 | App; ForgeNative already has `ForgePermissions.openSettings` |

### 05 Accessibility and ergonomics

| # | Finding | Apps | Priority | Where |
|---|---|---|---|---|
| X19 | Accessibility text sizes: Good Dog and Pocket Library switch row layout at AX sizes; Weather's 10-day rows use fixed widths that will clip; FieldKit's trip row (V5) wraps unevenly; the rest are unchecked | Weather, FieldKit, others | P2 | App; a baseline at `-ax5` per app (visual regression already supports variants) |
| X20 | Reduce Motion is read only by Weather. Little else animates, so the risk is low today; it grows with phase 3 | All | P3 | Docs |
| X21 | One-handed use: primary actions sit at the top right in every app (the `+`, Record, New Sketch). That is the iOS convention and fine for occasional actions; for an app's main repeated action (FieldKit Record, Checklist add, Chat send) the bottom of the screen is better, and Chat and Checklist already put it there | FieldKit | P2 | App |
| X22 | No VoiceOver journey has been run on the phone for any app; identifiers and labels are present, the experience is unproven. The ax spike can walk a screen, not judge it | All | P1 for phase 5 | Phase 5 sitting |
| X23 | Keyboard: only Chat has a keyboard shortcut. Low priority on iPhone; it matters only if iPad comes back | All | P3 | Record |

### 06 Product experience and coherence

| # | Finding | Apps | Priority | Where |
|---|---|---|---|---|
| X24 | First run asks for infrastructure: Chat (server URL and a `make` command), Soundscape Listen (take server), Relay (pairing before anything else). These are honest for a demo but block "understand it without being taught" | Chat, Soundscape, Relay | P1 | App (sensible defaults, sample content, or a plain-language first screen) |
| X25 | Empty apps teach nothing. Commonplace, FieldKit, Sketchbook, Checklist and Pocket Library start empty with a good one-line explanation and one action. None offers sample content or a guided first step, which most mature apps in these categories do (a starter feed list, a sample sketch) | Five apps | P2 | App |
| X26 | The apps share one ecosystem only through Device Lab. There is no shared voice guide (Good Dog's tone is the best and is not written down), no shared notion of an app's accent, and no rule for which pattern each app uses where two native ones exist (alert vs sheet for rename, tab vs sheet for settings) | All | P2 | Docs (phase 2 design direction) |

## App by app

Each app has: **what it's trying to be** (so phase 2 respects its personality), the **three questions** with a short verdict, and its findings. Findings already covered above or in the friction audit are only referenced.

### Chat

**Trying to be:** a private, everyday messenger for a small group, with an on-device assistant on the side. It should feel immediate and trustworthy: you always know whether your message got there. Personality: quiet and dependable, closer to Signal than to a social app.

| Question | Verdict |
|---|---|
| New user | **No.** The first screen is "Welcome to Chat" with a name field and a server URL field whose footer says to run `make chat-server`. Nothing says what Chat is or who you'd talk to |
| Experienced user | **Mostly.** Reply, react, edit, delete, voice messages, photos, documents, search, mute and swipe to mark read are all there. Delivery states (Sending, Delivered, Not Delivered, tap to try again) are clear |
| Polish | **Partly.** Send and failure haptics exist. No theme, no identity; bubbles use the default accent at 60% for sending, red for failed |

| # | Dimension | Finding | Reference | Priority | Where |
|---|---|---|---|---|---|
| C1 | Product | First run is a server form (X24). A hosted default (`ChatServers.hosted` exists but is nil) plus "Advanced: use your own server" would make the first screen a name and a button | Signal: a phone number and nothing technical | P1 | App |
| C2 | Product | Sign Out loses the account for good (F5, wording fixed). There is still no way back in; the real fix is sign-in or account export | Signal: re-register, transfer | P2 | App + Rust |
| C3 | Visual | No `forgeTheme`, no accent of its own; Chat is the flagship and looks the most generic | | P1 | App (X2) |
| C4 | Interaction | Reactions are a fixed emoji list in a context menu; the most common reaction takes a long press and a menu scroll | Signal, iMessage: a reaction bar above the bubble on long press | P3 | App |
| C5 | Feedback | Delivery is text (Sending, Delivered, Read, Not Delivered) under the newest message only. Clear, but wordy in a busy room | Signal: compact delivery marks, with detail on tap | P3 | App |
| C6 | Navigation | Assistant is a full tab, though it is unrelated to conversations; on a phone without Apple Intelligence the tab disappears and the bar changes | | P3 | App (decide whether Assistant is a tab or a compose option) |
| C7 | Accessibility | Tab items have no identifiers (X8); the attachment tiles use fixed 32 pt frames (not controls, fine) | | P3 | App |

### Good Dog

**Trying to be:** a kind, encouraging coach for training your dog in a few minutes a day. It should feel warm, calm and never judgmental. Personality: friendly and gentle; the copy carries it today, the visuals don't yet.

| Question | Verdict |
|---|---|
| New user | **Yes.** The welcome asks about your dog and the time you have, then shows today's plan. The best first minute in ixOS |
| Experienced user | **Yes, with density.** Today is a long grouped list: progress, rhythm, goal, activities, then "Only a Few Minutes Today" and "Not Today". The activity rows carry title, role, minutes and why, which is a lot to scan daily |
| Polish | **Mostly.** Haptics on selection and success, a stage celebration, a guided session. Visually it is a standard grouped list in the standard blue, which undersells a warm app |

| # | Dimension | Finding | Reference | Priority | Where |
|---|---|---|---|---|---|
| G1 | Visual | The warmest app wears the coldest default. An accent and a few illustrative symbols would carry the personality the copy already has (X2) | Duolingo-style coaching apps: colour as encouragement | P2 | App |
| G2 | Visual | Two equally prominent full-width buttons on the exercise screen (V1) | | P2 | App |
| G3 | Navigation | Today mixes four kinds of thing in one list (status, goal, plan, day choices); the plan, the thing you came for, is the third section | | P2 | App |
| G4 | Interaction | Guided session: the screen locks mid-session (F18) and Close ends it with no confirmation (F19). Hands are busy with the dog | | P1 | App (F18 is the second use of keep-screen-on, so the helper can be extracted) |
| G5 | Feedback | Completing an activity shows a checkmark in the row; the celebration is reserved for stages. A small moment for the daily plan being finished would fit the app | | P3 | App |

### FieldKit

**Trying to be:** a field notebook: press record, walk, note things, and come home to a map and the numbers. It should feel rugged and glanceable, usable with one hand outdoors. Personality: practical, map-first.

| Question | Verdict |
|---|---|
| New user | **Yes.** "No Trips" explains it and offers Record a Trip |
| Experienced user | **Mostly.** Starting a trip is Record, then a sheet with a pre-filled name, then Start: one tap more than needed for the main action. Background recording is now on (#256) |
| Polish | **Partly.** The live map and stats are clear. Warnings are raw orange (V3); the trip list rows wrap (V5); the trip screen is a list with a map on top rather than a map-led layout |

| # | Dimension | Finding | Reference | Priority | Where |
|---|---|---|---|---|---|
| K1 | Interaction | Record opens a naming sheet before anything records. Start with the default name and rename later (rename exists) | Workout and hiking apps: one big Start | P1 | App |
| K2 | Ergonomics | Record is a small toolbar icon at the top right; during a trip, Stop and Add Note are at the bottom, which is right. Make Record a large bottom button on the list too (X21) | | P2 | App |
| K3 | Visual | The trip list has no map thumbnail or distance emphasis; every trip looks the same | | P3 | App |
| K4 | Interaction | Trip delete is swipe-only (X11) | | P3 | App |
| K5 | Feedback | No pause; a coffee stop counts as moving time | | P2 | App + Rust |

### Soundscape

**Trying to be (from #269 on):** a music player for Reuben's own generated songs, with ways to compare takes and pick favourites. It should feel like a good music app: artwork, a queue, Now Playing, lock-screen controls. Personality: warm, immersive, audio-first. Today's main still carries the Studio (projects, tracks, mixer), which #269's series removes.

| Question | Verdict |
|---|---|
| New user | **No, today.** Listen needs a take server address first; Projects opens a recording studio. After #269 the library is the first screen, which is the right answer |
| Experienced user | **Yes for Listen.** Picks, stars, notes, A/B and lock-screen playback passed on the phone (2026-11-16) |
| Polish | **Partly.** Generated gradient artwork, mini player and Now Playing exist. Fixed font sizes (X5), raw orange and red (X4), and no interruption handling (F8, fix queued) |

| # | Dimension | Finding | Reference | Priority | Where |
|---|---|---|---|---|---|
| S1 | Product | First run asks for a take server. Remember the last one and prefill the laptop's address (this is a personal app) | Apple Music: content first | P1 | App (in the #269 series) |
| S2 | Visual | Transport icons at 64 pt fixed; they won't follow Dynamic Type or Bold Text | Apple Music | P2 | App |
| S3 | Feedback | Audio interruptions leave the transport wrong (F8) | | P1 | App (queued) |
| S4 | Interaction | Stars at caption size in orange; small targets for a frequent action | | P2 | App |

### Relay

**Trying to be:** "Your devices. One network." Moving a file to your own other device, or to someone nearby, should be as simple as AirDrop and feel safe. Personality: trustworthy and quick; security is felt, not explained.

| Question | Verdict |
|---|---|
| New user | **Partly.** The empty state explains pairing by QR and offers Show My Code. Sending to someone without pairing (word codes) is hidden in a `+` menu labelled Pair a Device (V7); #268 adds Send and Receive screens, which should answer this |
| Experienced user | **Partly.** A paired device is one tap away and history is in the same list. Both devices must have Relay open on the same Wi-Fi |
| Polish | **Partly.** Success haptic, Quick Look preview. A bare pencil renames this device (V7); transfer rows use raw green and accent |

| # | Dimension | Finding | Reference | Priority | Where |
|---|---|---|---|---|---|
| R1 | Navigation | The app is organised around devices; the task is "send this" or "receive". Lead with Send and Receive, with nearby and paired devices as targets | LocalSend: Receive, Send, Settings | P1 | App (check #268 first) |
| R2 | Feedback | Progress and failure wording: failed rows show Rust diagnostics (F10); large-file sends freeze (F9, fixed by #282's `ForgePickedFile`) | LocalSend: per-file progress, clear accept/decline on the receiver | P2 | App |
| R3 | Feedback | "Open while Relay is on screen" is the real constraint and is in a footer; people will send and lock the phone | | P2 | App (say it where the send happens) |
| R4 | Interaction | Forget is swipe-only (X11) | | P3 | App |

### Sketchbook

**Trying to be:** a pocket drawing notebook: open, draw, done. It should feel tactile and calm, with the canvas as the whole point. Personality: paper and ink.

| Question | Verdict |
|---|---|
| New user | **Yes.** "No Sketches" with New Sketch; the paper choice is a long press on `+`, so the default is one tap |
| Experienced user | **Mostly.** PencilKit tools, named undo and redo, layers, export with preview |
| Polish | **Mostly.** Paper backgrounds give it character. The canvas sits inside a navigation stack with a bar; there is no focus mode |

| # | Dimension | Finding | Reference | Priority | Where |
|---|---|---|---|---|---|
| K-S1 | Navigation | No full-screen drawing; the navigation bar and tool picker take a large share of a phone screen | Apple Notes / Freeform: hide chrome while drawing | P2 | App |
| K-S2 | Interaction | Inside Device Lab a downward stroke from the top can close the app (D1, confirmed on the phone) | | P1 for testing | Device Lab |
| K-S3 | Visual | Paper colours are fixed RGB with no dark variant; a white page in dark mode is a deliberate choice worth stating | Paper apps keep the page light | P3 | App (decide and document) |

### Weather

**Trying to be:** glanceable weather for here and the places you care about. It should feel atmospheric and instant. Personality: the most designed app in ixOS, a drawn sky per condition.

| Question | Verdict |
|---|---|
| New user | **Yes.** It shows My Location's weather straight away, or a clear empty page with Use My Location |
| Experienced user | **Yes.** Swipe between places, pull for the 10-day and details |
| Polish | **Yes, visually.** But it is very close to Apple's own Weather layout (big temperature, hourly strip, 10-day bars, tiles, page dots), so its strongest look is not its own |

| # | Dimension | Finding | Reference | Priority | Where |
|---|---|---|---|---|---|
| W1 | Visual | The layout follows Apple Weather closely. The README says it is "ixOS's own design"; phase 2 should decide what Weather's own idea is (for example, a plain-language "what to wear / will it rain on my walk" lead) rather than polishing a copy | Open-source weather apps (references.md) | P2 | App (design direction) |
| W2 | Accessibility | Fixed column widths in day rows and fixed 19 pt toolbar icons (X5, X19) | | P2 | App |
| W3 | Feedback | Location failures and staleness: F1 and F14/F15 fixed or in #283 | | done | |
| W4 | Product | No weather warnings; an atmospheric app that misses the one thing that matters on a bad day | Apple Weather, Met Office | P3 | App + Rust (data source permitting) |

### Commonplace

**Trying to be:** a calm, local-first reading place for the web you choose: feeds now, a full personal reading system later (decided 2026-11-16). It should feel quiet and focused, like a good reader. Personality: bookish and unhurried.

| Question | Verdict |
|---|---|
| New user | **Partly.** "No Feeds Yet" explains feeds and offers Add Feed, but needs a feed URL; a site address fails (F11). No suggested feeds |
| Experienced user | **Partly.** Smart lists, unread counts, swipe read and bookmark, search, ForgeReader text settings. No next/previous article in the reader, so reading a timeline is back-and-forth |
| Polish | **Mostly.** ForgeRow tones on the smart lists, a status line in the bottom bar (V4: may truncate) |

| # | Dimension | Finding | Reference | Priority | Where |
|---|---|---|---|---|---|
| P-C1 | Interaction | No next article / next unread in the article screen | NetNewsWire: next unread, swipe between articles | P1 | App |
| P-C2 | Product | Feed discovery from a site address (F11) and a few suggested feeds on first run | NetNewsWire: discovery, defaults | P1 | App + Rust |
| P-C3 | Interaction | Mark All as Read confirmed (#274); an undo would be lighter (X10) | NetNewsWire: Mark All as Read with undo | P3 | ForgeAppKit (X10) |
| P-C4 | Visual | Article list rows are dense text; no thumbnails or feed icons | NetNewsWire: feed favicons | P3 | App |

### Checklist

**Trying to be:** the quickest list on the phone: type, tick, done. It is growing into due dates, recurring tasks and reminders (decided 2026-11-16). Personality: crisp and fast.

| Question | Verdict |
|---|---|
| New user | **Yes.** An add field at the top and "Nothing to do" |
| Experienced user | **Partly.** Adding keeps focus for the next item, which is right. Done items stay in place, struck through, mixed with open ones; no reordering; rename and delete are swipe-only |
| Polish | **No.** No ForgeUI, no theme, no haptics. Share opens a half sheet with one button before the system share sheet (V8) |

| # | Dimension | Finding | Reference | Priority | Where |
|---|---|---|---|---|---|
| L1 | Interaction | Swipe delete with no confirmation and no undo (X10) | Reminders: undo via shake; Things: undo banner | P2 | ForgeAppKit (X10) |
| L2 | Interaction | Done items mixed with open ones; no "Completed" section or hide-done | Reminders: Show/Hide Completed | P2 | App |
| L3 | Feedback | Ticking an item has no haptic or animation; the app's single most common action feels flat | Things, Reminders | P2 | App |
| L4 | Visual | Not on ForgeUI or a theme at all | | P2 | App |
| L5 | Interaction | Share sheet in front of the share sheet (V8) | | P3 | App |

### Pocket Library

**Trying to be:** a personal reading life: what you're reading now, what's next, and a good place to read and listen. It is also ixOS's reference app. Personality: cosy and literate; covers carry it.

| Question | Verdict |
|---|---|
| New user | **Yes.** An empty library with an Add menu (catalogue, manual, EPUB) |
| Experienced user | **Partly.** Continue Reading is three levels down (Library, the book, Continue Reading); Reading Now lives under Activity |
| Polish | **Yes, mostly.** Generated cover placeholders, read-aloud with lock-screen controls, summaries, AX layouts. The stars are now a toned colour (V2 fixed) |

| # | Dimension | Finding | Reference | Priority | Where |
|---|---|---|---|---|---|
| PL1 | Navigation | Put the current book(s) at the top of Library with a one-tap Continue; Activity keeps the stats | Apple Books: Reading Now first | P1 | App |
| PL2 | Navigation | Settings is a full tab (X7) | | P3 | App |
| PL3 | Interaction | Catalogue: only the small `+` adds a book (F20) | | P2 | App |
| PL4 | Product | As the reference app it should be the first app to adopt every phase 3 pattern, and its README screenshots placeholder is still empty | | P2 | App |

### Device Lab

**Trying to be:** two things. For Reuben, the one app on the phone that hosts every ixOS app for testing. For a developer, a live readout of what an iPhone tells an app. Personality: an instrument panel: dense, precise, honest about permissions.

| Question | Verdict |
|---|---|
| New user | **Partly.** The dashboard is clear as an instrument panel; the apps it hosts are behind a toolbar icon |
| Experienced user | **Partly.** Open at Launch is a good shortcut. Opening a different app means swipe down, Apps, pick |
| Polish | **Mostly.** Information is dense and well labelled; raw colours (13) and a fixed compass font |

| # | Dimension | Finding | Reference | Priority | Where |
|---|---|---|---|---|---|
| D-1 | Navigation | Apps first: make the app list the root (or a tab next to Device) since it is what the phone is for now | Apple's developer tools: progressive disclosure, task first | P1 | Device Lab |
| D-2 | Navigation | Full-screen app presentation with a visible way back (X9, D1, D3) | | P1 | Device Lab |
| D-3 | Product | "This Build" is the only proof of which build is installed; keep it one tap from anywhere | | P3 | Device Lab |

### The starter (`forge init`)

**Trying to be:** the example every new app copies. Whatever it does, the next app does.

It uses ForgeUI, `.forgeTheme(.standard)`, `ForgeStateView` and ForgeAppKit, so new apps start better than the old ones. Two gaps carry straight into every new app: no placeholder icon (X1), and the standard accent with no prompt to choose one (X2). When phase 3 adds patterns, the starter adopts them in the same PR (rule 12, app parity).

## Shared gap analysis: what goes where

The package strategy decides the home of each piece, not habit. ForgeUI is the standalone SwiftUI design system (no Rust, no AppKit); ForgeAppKit is the optional layer of app patterns on top of it.

| Need | Home | Why there | Evidence (second use) |
|---|---|---|---|
| Per-app accent and identity | ForgeUI (exists: `withAccent`) + each app | Already possible; nobody uses it | All apps |
| Named text roles (row title, metadata, section note) over system styles | ForgeUI | Pure presentation, useful to any SwiftUI app | All apps vary |
| Inline notice (offline, stale, permission) | ForgeUI | A component; Weather's `Notice` and Chat's banner are two uses | Weather, Chat, Commonplace |
| Undo after a destructive action | ForgeAppKit | A model value with an action and a timeout, like `ForgeAlert` | Checklist, Commonplace |
| Search state (debounce, in-flight, no results) | ForgeAppKit | Behaviour, not look; four apps re-wrote it | Chat, Commonplace, Checklist, Pocket Library |
| Rename with validation | ForgeAppKit | Five apps, two shapes, none validates in place | Commonplace, Sketchbook, Relay, FieldKit, Checklist |
| Keep the screen on during a task | ForgeAppKit (or ForgeKit service) | Second use | Pocket Library, Good Dog |
| Success feedback (haptic plus optional animation on a model counter) | ForgeAppKit | Four apps use a `saveCount` trigger by hand | FieldKit, Relay, Sketchbook, Good Dog |
| Settings placement, rename pattern, when to confirm vs undo | Docs (ios-ui.md) | Rules, not code | All |
| Toned text, fixed font sizes, swipe without menu, tab identifiers | Check (parity rules) | Cheap to grep, keep coming back | See X4, X5, X8, X11 |
| Accessibility tree in baselines | forge-uitest (exists: `.tree.json` saved with every screenshot) | Diff it like the pixels | Pocket Library baselines |

Not proposed: a button, card or list component library beyond what ForgeUI has; a navigation framework; custom fonts; one look for every app.

## Proposed phase 2 direction

A short design-direction doc, not a redesign:

1. **Identity kit per app:** an icon, an accent (checked by `contrastIssues()`), a symbol set, and one line of personality from this audit. This is the cheapest, most visible change and needs no framework work.
2. **Five interaction principles** for ixOS, each with the native pattern that implements it: the main task is one tap from launch; undo frequent actions, confirm rare costly ones; every action ends in visible or felt feedback; first run works without infrastructure or shows sample content; swipe actions always have a visible twin.
3. **A small foundations list** for phase 3, in priority order: text roles and the inline notice (ForgeUI); undo, search state and success feedback (ForgeAppKit); the parity checks above. Pocket Library adopts first, the starter in the same PR.
4. **Order of app redesigns** by use, not ease: Device Lab's Apps-first and full-screen hosting (it unblocks honest phase 5 testing), then Pocket Library, Chat, Commonplace, Good Dog, Checklist, FieldKit, Relay (after #268), Soundscape (after the #269 series), Sketchbook, Weather.

## Decisions for Reuben

1. **Where shared pieces live.** Recommendation: look in ForgeUI, behaviour in ForgeAppKit, as the package strategy says, rather than putting the whole Experience System in ForgeAppKit. This keeps ForgeUI usable by any SwiftUI app.
2. **App icons and accents:** who makes the icons (you, a designer, or generated placeholders for now)? Names are provisional, so icons can be too. Recommendation: simple generated placeholders now, real ones before any App Store release.
3. **Undo vs confirmation** as an ixOS rule (principle 2 above). Recommendation: yes.
4. **Weather's identity:** keep refining the Apple-like layout, or give it its own idea? Recommendation: its own idea, decided in phase 2.
5. **Reference for Pocket Library.** NetNewsWire is a feed reader, so it is Commonplace's natural reference (it was already its benchmark, #148). Recommendation: NetNewsWire for Commonplace, Apple Books (behaviour only) for Pocket Library. See [references.md](references.md).
6. **Device Lab full-screen hosting** before phase 5. Recommendation: yes, small and first.

## Proposed phone sitting (about 10 minutes)

Not blocking; it turns source findings into phone evidence where source can't decide. On the Device Lab build already installed, nothing to build first.

1. **Liquid Glass, 1 min.** Open Pocket Library: is the tab bar the iOS 26 glass bar or the older opaque one? Linux xtool builds may run in compatibility mode ([forge-ui-testing.md](../../forge-ui-testing.md)). This decides what "native" looks like for the whole design direction.
2. **Largest text, 3 min.** Settings, Accessibility, Larger Text, the largest size. Open Weather (10-day rows), FieldKit (trip list), Checklist and Chat. Screenshot anything clipped.
3. **VoiceOver, 4 min.** VoiceOver on, Checklist: add an item, tick it, delete it. Then Pocket Library: continue a book. Proves whether the two simplest journeys work by ear.
4. **Home screen, 30 s.** A screenshot of Device Lab's icon, for the "before" of X1.

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
| App icon set | no | no | no | no | no | no | no | no | no | no | no | no |
