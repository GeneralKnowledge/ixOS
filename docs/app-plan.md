# What each app builds next, and what the framework needs for it

Asked for by Reuben on 2026-11-21. One page that says, for every app, what to build or add next, and which framework parts (ForgeUI, ForgeAppKit, the `forge-app` Rust runtime, native services) those features would need. It is a plan, not a commitment: each item still goes through the [completeness loop](roadmap.md#apps-first-prove-complete-then-extract), one feature at a time, compiled for the iPhone and checked on it before the app's next pick.

Where an app already has its own plan, this page summarises it and links it instead of planning it again.

## The rules this plan follows

- **Complete, not bigger.** Each app's list starts with proving what is already merged, then the most noticeable thing a user would miss. No new apps ([roadmap](roadmap.md#apps-first-prove-complete-then-extract)).
- **Expand an existing app before adding a new one.** The capability pairs (notifications, recurring schedules, calendar, widgets, App Intents, timeline, background work) decide which app goes first.
- **Apps first, then extract.** The first implementation stays in the app. A capability moves into the framework only when a second app needs it, and only after both apps' features have passed on the phone.
- **Wrap Apple's frameworks, never replace them.** Swift Charts, EventKit, WidgetKit, App Intents, BackgroundTasks, AVFoundation, Speech and PhotosUI do the work; ixOS adds the Rust rules and the shared styling.
- **Check ForgeUI and ForgeAppKit first.** A feature that builds its own control says why in its PR ([ForgeUI coverage audit](research/experience/forgeui-coverage.md)).

**Paid Apple account.** Items marked **(paid)** need the paid Apple Developer Program; the rest work on the free team (3 sideloaded apps, 7-day signing, 10 new App IDs a week). Items marked **(spike)** might work on the free team and need a short test first. The full list is in [What a paid account would unlock](#what-a-paid-account-would-unlock).

## Status at a glance

"Step 0" for every app is the same: the checks already built and waiting for a phone sitting in the combined Device Lab build. They come before any new feature in that app.

| App | Step 0: waiting for the phone | First new feature |
|---|---|---|
| Relay | Relay 1.0 acceptance run, My Devices step 1 | N1 My Devices: signed device list |
| Good Dog | Goals, lessons, safety rules, guided session, reminders (GD2b, GD5, GD-7) | Progress over time (GD4) |
| FieldKit | Recording with the phone locked | Photo and voice notes (F2c) |
| Soundscape | Takes, gapless and crossfade, sleep timer, saved queue, Identify Song, Archive | Fixed test catalogue (N4), then resolver |
| Sketchbook | First run ever on an iPhone (S2) | Nothing new until it passes |
| Weather | W1 passed 2026-11-21 | W2 alerts and next-hour rain (paused) |
| Commonplace | C3 folders and OPML, C4 full articles | C5 per-feed settings and filters |
| Checklist | Search; due dates and the agenda (built 2026-11-22) | Reminders for due items |
| Pocket Library | The P4 sitting (PL-1 to PL-10), Read Aloud chapters | Full-text search in books and notes |
| Urdu, Naturally | 0.1 (M1) | M2 free conversation |
| Sums | R2: replay 39 sums, ask Calculator 16 new ones | R3 compare screens |
| AI Lab | Bonsai Harness H1 to H3 (#357) | H4 planning |

## The apps

### Relay

Its own product track. Plans: relay-network-architecture.md ([#352](https://github.com/GeneralKnowledge/ixOS/pull/352), stages N1 to N9; link it here once merged), [relay-audit-2026-11.md](../examples/relay/docs/relay-audit-2026-11.md) (the 1.0 acceptance list), [relay-my-devices.md](../examples/relay/docs/relay-my-devices.md).

1. Relay 1.0 acceptance on the iPhone and Linux, then with several devices (audit step 5), plus the introducer check from #354.
2. **N1 My Devices**: a signed device list, so a person's devices are learnt, trusted and revoked once.
3. **N2 Away from home**: Wi-Fi, Tailscale and the Hetzner relay without manual networking.
4. **N3 Encrypted mailbox**: things wait while a device is away.
5. **N4 One messaging security model**: the chat server path moves onto Relay's identities and encryption.
6. N5 to N9 as the architecture doc orders them: capabilities and permissions, the first capability (Sites or transcription), groups on MLS, continuity, federation and agents.

Framework it touches: none new. Relay brings framework changes in only when it needs them. Its outbox is the first user of a durable work queue. A phone woken by a new message while Relay is closed needs push **(paid)**. Local network discovery by multicast needs the paid team and Apple's approval **(paid)**.

### Good Dog

Plan: [good-dog-demo.md](../examples/good-dog/docs/good-dog-demo.md) (milestones GD0 to GD9).

1. Step 0: set up a dog, start a goal, open a lesson, see a safety rule hold a stage, run the guided session, and get one reminder (GD2b, GD-7, GD5). None of these has been recorded on the phone yet.
2. **Progress over time (GD4)**: Swift Charts per goal (contexts practised, learned against reliable, sessions over the weeks). Streaks optional and never the main measure. The second user of the chart style, after FieldKit.
3. **Reminders finished (GD5)**: the week rolls on in the background, not only when the app opens. The first user of background scheduled work beside Commonplace.
4. **Reflections and adaptive steps (GD3)**: a quick record plus where it happened and signs of stress; the welfare wording reviewed first.
5. **Training sessions in the calendar**: add a planned session or a vet or trainer appointment to Apple's Calendar through EventKit's editor (no full calendar permission). First user of calendar integration.
6. **Widget and Live Activity (GD6)**: the goal's next step on the home screen; a running session on the lock screen. First user of widgets **(spike)**.
7. Trainer summary as CSV and PDF (rest of GD4); then GD7 (Good Dog knows the dog; Ask needs Reuben's go-ahead) and GD8 (everyday life, needs new content).

### FieldKit

Plans: [FieldKit README](../examples/fieldkit/README.md), [Android ports plan, OpenTracks](research/android-ports.md#2-opentracks--fieldkit) (T1 to T5).

1. Step 0: record with the phone locked (not yet reported).
2. **Photo and voice notes (F2c)**: PhotosPicker and the camera for photos, a voice memo for audio. The second user of the photo picker after Relay.
3. **Background recording**: keep recording with the screen off, with the location service's background mode.
4. **T1 import and activity types**: GPX and KMZ import with duplicate detection, totals by type.
5. **A timeline of a trip**: notes, photos and track in time order. The second user of timeline presentation, after Commonplace.
6. **Planned trips in the calendar** through EventKit. The second user of calendar integration, after Good Dog.
7. T2 KML and automatic export, T3 spoken announcements, then T4 Bluetooth heart rate (needs a real strap) and T5.

Writing workouts to Apple Health is possible later (HealthKit on a free team is unproven, so **(spike)**).

### Soundscape

Plans: [architecture.md](../examples/soundscape/docs/architecture.md#work-in-order) (steps 1 to 12), [sources.md](../examples/soundscape/docs/sources.md), [Android ports plan, AntennaPod](research/android-ports.md#3-antennapod--soundscape) (P0 to P4), [podcasts and live radio](../examples/soundscape/docs/podcasts-and-radio.md) (proposal). Player features come before more sources.

1. Step 0, **the one list of owed Soundscape phone checks** (other Soundscape plans point here; nothing new starts before it passes): takes (M5), gapless and crossfade, sleep timer, the saved queue after a restart, Identify Song, downloads and licences of catalogue tracks, Archive Add All, the playlist fixes from #328, the redesign's checks ([design.md](../examples/soundscape/docs/design.md), #418), Smart Playlists (#428), Jamendo artist and release pages with functional albums (#444, #445), and no silence when changing song (#447). Each is recorded as it passes; none is assumed.
2. **A small fixed test catalogue (N4)**, so phone checks don't depend on Jamendo's charts.
3. **Stable recording ids and the resolver** (steps 5 and 6): the player asks Rust what to play and shows why when nothing can.
4. **Reconciliation**: linked songs merge into one song with several sources.
5. **P0 player features**: playback speed, skip amounts, a saved position per item.
6. **Background downloads and a storage limit** (step 10): the first real feature on the background transfers service, and everyday capabilities phase 3 (kept but not backed up).
7. Loudness levelling (after measuring on the phone), then podcasts (P1 to P4) as their own section. Live radio (L1, L2) is proposed to come after step 0 and before P0; see [podcasts-and-radio.md](../examples/soundscape/docs/podcasts-and-radio.md).

Syncing the library between devices (step 12) could go over Relay or iCloud **(paid)**; no decision needed yet.

### Sketchbook

Plan: [Sketchbook README](../examples/sketchbook/README.md). No new features until its first run on the iPhone.

1. Step 0: new sketch, draw with the tool picker, add a layer, export PNG and Share.
2. **Unreadable sketches have a way forward**: everyday capabilities phase 1 (Start Fresh for damaged data, never for newer data), proven here first, then Pocket Library.
3. **Import a photo as a layer** to trace over. A third user of the photo picker.
4. **Apple Pencil and iPad layout**, only if Reuben wants Sketchbook on the iPad.

### Weather

Plan: [breezy-counterpart-plan.md](../examples/weather/docs/breezy-counterpart-plan.md), phases W1 to W6. W1 passed on the phone on 2026-11-21.

1. **W2 alerts and next-hour rain** (paused until Reuben says go). Alerts are safety-critical (safeguard B). A local notification for a new severe alert.
2. **W3 more sources and a choice per place**: MET Norway, NWS, Bright Sky; bring-your-own-key sources with keys in the Keychain.
3. **W4 charts and details**: hourly and daily Swift Charts. The third user of the chart style.
4. **W5 widget and background refresh**: a widget **(spike)**, background refresh feeding the widget and alerts.
5. W6 community release: a decision, and needs the paid team for TestFlight or the App Store **(paid)**.

### Commonplace

Plans: [Commonplace README](../examples/commonplace/README.md), [Android ports plan, Feeder](research/android-ports.md#1-feeder--commonplace) (C3 to C6). Commonplace is the feed reader; there is no other.

1. Step 0: C3 folders and OPML (import, see the folders, export, re-import into a fresh library) and C4 full articles.
2. **C5 per-feed settings and filters**: notify, full text, how to open, a block list, refresh only on Wi-Fi, keep the last N articles.
3. **Background refresh**: new articles fetched while the app is closed, as iOS allows (BGAppRefreshTask). The first user of background scheduled work.
4. **C6 read aloud**: the third user of speech, after Pocket Library and Urdu, Naturally, which makes ForgeSpeech's extraction real.
5. **Add from the share sheet**: a share extension to add a feed or save a page from Safari. Needs App Groups **(spike)**.
6. **Daily notes and a reading queue with follow-up dates**: Commonplace organising information over time. The first user of timeline presentation.

### Checklist

The smallest app on the runtime ([README](../examples/checklist/README.md)). It has no plan of its own; this is its list. It is the first user of recurring schedules and App Intents, and the second of notifications and widgets.

1. Step 0: search (ticking and Clear Done Items passed on 2026-11-16).
2. **Due dates and an agenda**: Today, Upcoming, Overdue, all in Rust. Built on 2026-11-22 (CL-4); waits for the phone.
3. **Reminders for due items**: local notifications, replaced when a date changes. The second user of notifications after Good Dog, and the evidence for whatever they share.
4. **Recurring items**: every day, week or month, and "after done". The first user of recurring schedules; Good Dog's training rhythm is the second.
5. **Several lists**, each with its own items.
6. **Shortcuts and Siri**: "add to my list", "what's due today" with App Intents ([app-intents-on-linux.md](research/app-intents-on-linux.md)). The first user of App Intents.
7. **A widget** of what's due today **(spike)**, the second user after Good Dog.

### Pocket Library

The reference app. Plans: [README](../examples/pocket-library/README.md), [ACCEPTANCE.md](../examples/pocket-library/ACCEPTANCE.md), and the [Android parity plan](roadmap.md#android--work-in-progress) (A1 to A8; A1 to A5 done, A6 in progress).

1. Step 0: the P4 sitting, every requirement PL-1 to PL-10 on the iPhone, and Read Aloud's chapters and lock-screen details.
2. **Full-text search inside books and notes**: matching passages, jumping to the place in the book, filters. Wave 5, step 1.2: the second user of full-text search after Commonplace.
3. **Reading goals and reminders to resume**: a yearly goal and a gentle reminder for a book left unread. A third user of notifications.
4. **Shortcuts**: "continue my book", "add a book" with App Intents. The second user after Checklist.
5. **Unreadable library has a way forward**: everyday capabilities phase 1, second app after Sketchbook.
6. **Save a book from the share sheet** (spike, as Commonplace) and shared book links that open the app (universal links **(paid)**).
7. Scan a book's barcode with VisionKit to look it up in the catalogue.

### Urdu, Naturally

Plan: [plan.md](../examples/urdu-naturally/docs/plan.md), milestones M0 to M7. Reuben uses it first; the next milestone starts when he asks.

1. Step 0: 0.1 (M1) on the phone, and which Urdu voices the phone has.
2. Small things he asked for after the build: the opposites exercise, free-sentence checking for patterns, nahi and na gap lines.
3. **M2 free conversation with Ali, Ayesha and Omar**, on Apple's on-device model first, behind a no-AI setting.
4. **M3 native-speaker review**, with a reviewer mode; nothing is marked reviewed without it.
5. M4 Before and After You Talk, M5 Sound Lab (needs recording yourself), M6 listening four ways, M7 the personal profile and phrasebook.

Speech recognition (whether SFSpeechRecognizer lists Urdu) decides how speaking works; it would also be Relay's voice messages' first transcription.

### Sums

Plan: [app-as-specification.md](research/app-as-specification.md) and the [Calculator expedition](research/expeditions/calculator/profile.md).

1. Step 0, R2: replay the 39 sums on the phone, ask Calculator the 16 held-out ones ("full" then back to operate), and compare.
2. **R3: compare screens automatically**, which also decides whether the explorer comes back (it lives in forge-lab).
3. Fixing what the comparison finds. Sums gets no features of its own beyond the reference's behaviour; landscape is ignored.

### AI Lab (Bonsai Harness and Python Lab)

Plan: bonsai-harness-plan.md ([#357](https://github.com/GeneralKnowledge/ixOS/pull/357), stages H0 to H8). It lives in Device Lab until a second environment exists.

1. Step 0: H1 to H3, one task end to end (hello, one test, a seeded bug fix) on the phone.
2. **H2 a reliable model interface**, H4 planning and several tasks, H5 recovery and replanning, H6 context management.
3. H7 a small multi-file project, then H8 the benchmark (his expenses-app task word for word).

Python Lab itself: [python-lab-workspace-plan.md](../examples/device-lab/docs/python-lab-workspace-plan.md), stages PL1 (workspace and interactive console, which also delivers H7's Python Lab half) to PL4.

Framework it touches: llama.cpp stays in Device Lab (`LlamaCppModel`) until a second app runs GGUF models; Urdu, Naturally's M2 is the likely one, if Apple's model can't hold Urdu.

### Other tracks, summarised

- **Android**: Pocket Library only, one milestone at a time; A1 to A5 are done (PL-1 to PL-10 on the emulator) and A6 (background work, settings, backups) is in progress ([roadmap](roadmap.md#android--work-in-progress)).
- **Android ports**: Feeder into Commonplace, OpenTracks into FieldKit, AntennaPod into Soundscape, in that order, clean room for GPL apps ([android-ports.md](research/android-ports.md)). Their phases are folded into the app lists above.
- **VPN app with eSIM and Travel Hub**: a separate product in the VPN repository, not part of this plan. Its native tunnel needs a paid organisation team **(paid)**.

## The framework those features need

Each row is one capability. "Where it goes" names the package: ForgeUI (SwiftUI design system), ForgeAppKit (app patterns), `forge-app` / `forge-runtime` / `forge-storage` / `forge-net` (Rust), or `forge-services` (native services: a Rust interface only when Rust reads the value, otherwise a Swift helper over Apple's framework). "When" is the extraction gate: what has to pass on the phone first.

### Already extracted, waiting for the phone

| Capability | Apps | Where | Waiting for |
|---|---|---|---|
| Now Playing and audio rules (`ForgeNowPlaying`, `ForgeAudioRules`) | Soundscape, Pocket Library | `forge-services` Swift helpers | A phone check of both |
| Swipe guard and form toolbar | Every app with a sheet | ForgeUI | The checks in the combined build |
| Web images, Undo banners, haptics | Several | ForgeUI, ForgeKit | Done |

### Next extractions (a second user exists or is planned)

| Capability | First app | Second app | Where | Extract when | Paid? |
|---|---|---|---|---|---|
| **Photo picker** (a picked photo as a temporary file Rust reads) | Relay | Soundscape (artwork), then FieldKit, Sketchbook | `forge-services`, next to `ForgePickedFile` | Soundscape's artwork picker passes on the phone (the next extraction in the queue) | No |
| **ForgeSpeech** (voices, best-voice choice, speaking with word highlights) | Pocket Library Read Aloud | Urdu, Naturally; Commonplace C6 third | `forge-services` Swift helper | Urdu passes on the phone | No |
| **Chart style** (Swift Charts styled from ForgeUI tokens, VoiceOver summaries) | FieldKit | Good Dog GD4, then Weather W4 | ForgeUI | Good Dog's progress passes | No |
| **Full-text search** (FTS5 index, snippets, cancellation) | Commonplace | Pocket Library | `forge-storage` | Pocket Library's search passes; then compare and move only the shared part | No |
| **Unreadable data recovery** (typed open failure reaching `ForgeRoot`, Start Fresh) | Sketchbook | Pocket Library | ForgeAppKit + `forge-app` | Everyday capabilities phase 1, both on the phone | No |
| **Refused permission screen** (denied state, Settings link, re-check on resume) | FieldKit | Sketchbook, Relay | ForgeAppKit | Phase 2 | No |

### Capabilities the app lists will create (first user named, not extracted yet)

| Capability | First app | Second app | Where it would go | Extract when | Paid? |
|---|---|---|---|---|---|
| **Local notification schedules** (one per day, replaced on change; the service exists) | Good Dog | Checklist | Rust rules in `forge-app`; the `notifications` service as is | Checklist reminders pass | No |
| **Recurring schedules** (every day, week, month, after done; local days) | Checklist | Good Dog | `forge-runtime` (time) or `forge-app` | Good Dog uses it for its rhythm | No |
| **Calendar integration** (EventKit's editor, no full access) | Good Dog | FieldKit | `forge-services` Swift helper | FieldKit's planned trips pass | No |
| **Background scheduled work** (BGAppRefreshTask, an honest "when iOS allows") | Commonplace | Good Dog | `forge-services` `background` | Good Dog's rollover passes | No |
| **Widgets** (extension template, a small shared snapshot) | Good Dog | Checklist, Weather W5 | Template + `forge-app` snapshot contract ([app-model-review.md](app-model-review.md)) | Spike S2 first, then the second widget | Spike; paid removes doubt |
| **App Intents and Shortcuts** | Checklist | Pocket Library | Recipe first, then a helper | Pocket Library's shortcuts pass | No |
| **Timeline presentation** | Commonplace | FieldKit | ForgeUI | FieldKit's trip timeline passes | No |
| **Share extension** (receive URLs, text, files) | Commonplace | Pocket Library | Template + helper | Spike S2 (App Groups), then both | Spike |
| **Durable work queue** (stored before sending, retried, recovered) | Relay outbox | Soundscape uploads or podcasts | `forge-runtime` | A second app needs it; Relay's stays its own until then | No |
| **Background downloads with a storage limit** (the service exists) | Soundscape | Soundscape podcasts, Pocket Library | `forge-services` `transfers` + phase 3 storage class | Soundscape's downloads pass | No |
| **Feed parsing and OPML** | Commonplace | Soundscape podcasts (built in Soundscape first) | A small crate or `forge-net` module | Both Commonplace's feeds and Soundscape's podcasts pass on the phone | No |
| **Speech recognition** | Urdu, Naturally | Relay voice messages | `forge-services` optional | Both pass | No |
| **Keychain keys the person brings** (the Keychain service exists) | Weather W3 | Soundscape's Jamendo client ID | Recipe | Already shared through the service; nothing to extract | No |
| **GGUF models with llama.cpp** | AI Lab | Urdu M2 (if needed) | `forge-services` `ai` backend | A second app loads a model | No |
| **Bluetooth LE sensors** | FieldKit T4 | none yet | `forge-services` `bluetooth` | Stays in FieldKit until a second app | No |

### Not planned

A sync engine, a shared calendar or agenda screen, a charting engine, a speech engine of our own and a generic background executor. Each app's agenda, timeline or calendar screen stays its own design; only the plumbing is shared ([roadmap](roadmap.md#apps-first-prove-complete-then-extract), [native-capability-roadmap.md](native-capability-roadmap.md#not-planned)).

## What a paid account would unlock

| What | Apps that would use it | Without it |
|---|---|---|
| **Push notifications (APNs)** | Relay (wake for a new message or a mailbox item), Weather (alerts while closed) | Foreground delivery and local notifications only |
| **iCloud and CloudKit** | Checklist, Pocket Library, Soundscape library, Good Dog (the same data on every device) | Export and import, or sync over Relay later |
| **Network Extension** | VPN app's native tunnel (needs an organisation team); possibly Relay later | None for the VPN; Relay works without it |
| **Multicast discovery** | Relay (find devices on the local network) | Codes, trusted devices and the relay server |
| **Universal links** | Pocket Library shared book links, Relay invite links | Custom URL schemes |
| **App Groups and extensions with no slot or weekly App ID limit** | Widgets (Good Dog, Checklist, Weather), share extension (Commonplace, Pocket Library) | Possible on the free team: spike S2 decides; each extension uses one of 10 App IDs a week |
| **StoreKit, TestFlight and the App Store** | Weather W6, the first App Store release, the VPN and eSIM products | Sideloading by cable only |
| **NFC, Wi-Fi network name** | FieldKit (tags at sites), Device Lab | Not available |
| **No 3-app limit, no 7-day re-signing** | Every app could be installed on its own | Device Lab hosts every app |

None of the items above blocks the next feature of any app: every first pick in the lists runs on the free team.
