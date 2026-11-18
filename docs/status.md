# Status: what works today

<!-- Generated from docs/ledger/ by `forge ledger generate`. Edit the records there, not this page. -->

This page answers "does it work yet?" in one place. It is made from the same records as the [capability ledger](capability-ledger.md), which has every detail, and it follows the ledger's rule: something works on an iPhone only when a person ran an app that uses it on a physical iPhone and recorded the result. Built and tested on Linux is not enough. If another page's status note disagrees with this page, this page is the current one.

The newcomer path (set up Linux, `forge init`, `forge run ios`) is described in [getting-started.md](getting-started.md); the *Forge Device* and *Forge UI Testing* rows below say how far the tools for running and testing on a phone have been proven. Publishing to the App Store is not supported yet ([first-app-store-release.md](first-app-store-release.md)).

**Of 102 capabilities, 14 work on an iPhone, 27 partly work, 46 are built but haven't been run on an iPhone, and 15 aren't built yet.**

## Works on an iPhone

A person ran an app that uses it on a physical iPhone and recorded the result.

| Area | Capability | What was seen |
|---|---|---|
| Foundation | App-owned Rust crate with UniFFI async exports | Pocket Library (a `forge init` app with its own crate) on iPhone16,2, iOS 26.3.1, 2026-11-07: UI test m0 passed 11/11, watched by Reuben. A fresh app from the current starter has not been recorded on a phone |
| Data and files | JSON snapshot storage with migration hook | Chat, 2026-11-05 |
| Networking | HTTPS requests with typed errors | 2026-11-05 and 2026-11-06 |
| Networking | Long-lived streams (SSE) with reconnect | Chat |
| Extensions and restricted frameworks | Core ML | Qwen3.5 0.8B (copied in over USB) compiled on the phone and run, iPhone 15 Pro Max, iOS 26.3.1, 2026-11-07 |
| Forge Device | Phone details from Linux (`forge device list`, `info`) | iPhone16,2, iOS 26.3.1, 2026-11-07 |
| Forge Device | Live system log, filtered by app (`forge device logs`) | iPhone, iOS 26.3.1, 2026-11-07 (`--grep`, `--process`, `--limit`) |
| Forge Device | Crash reports (`forge device crashes`) | iPhone, iOS 26.3.1, 2026-11-07 (listed 161, pulled 4) |
| Forge Device | Symbolicated crash reports (`forge crash`, `forge device crashes --pull`, `forge_read_crashes`) | Cati, iOS 26.3.1, 2026-11-08: Device Lab debug build, crashes tapped by Reuben; Swift crash to LabModel.swift:61 and Rust panic to crash_test.rs:24, build matched by UUID, Swift demangled. Real reports of other apps parsed too |
| Forge UI Testing | Mount the Developer Disk Image and start the WebDriverAgent runner from Linux (`forge ui setup`) | iPhone16,2, iOS 26.3.1, WDA 16.15.2, 2026-11-07 (M0 run watched by Reuben) |
| Forge UI Testing | Launch an app, find elements by accessibility identifier, tap, type, read the UI tree (`forge ui run`, `tree`) | M0 passed 11/11 in 10 s, iPhone16,2, iOS 26.3.1, 2026-11-07 (watched by Reuben) |
| Forge UI Testing | Act in apps Forge didn't build, under a per-app policy (`forge ui --any-app`, `~/.config/forge/device-control.toml`) | iPhone16,2, iOS 26.3.1, 2026-11-08 (Reuben present): Pocket Library m0 11/11 without --any-app; Claude app typed, then Send stopped at the [y/N] prompt and was refused on n (journal: refused, presses_return false); refused with no terminal; refused without --any-app before any action. ax_probe Ctrl-C exited cleanly, but before the walk started, so cleanup mid-walk is unproven |
| Forge UI Testing | Screenshot saved on Linux (`forge ui screenshot`) | 1290x2796 PNGs from M0, iPhone16,2, iOS 26.3.1, 2026-11-07 |
| Forge UI Testing | Accessibility identifiers and labels in the demos | Pocket Library ids (library.*, bookForm.*, tab.*) found through WDA, iPhone16,2, iOS 26.3.1, 2026-11-07 |

## Partly works on an iPhone

Some of it was seen working on an iPhone. The note says what was seen and what wasn't.

| Area | Capability | What was seen |
|---|---|---|
| Foundation | Native SwiftUI structure (navigation, lists, sheets, search) | Pocket Library list, add sheet and form on iPhone16,2, iOS 26.3.1, 2026-11-07 (UI test m0 11/11, watched by Reuben); navigation pushes and search not exercised |
| Foundation | ForgeUI design system (tokens, light/dark themes, styles and components on SwiftUI controls) | ForgeUI showcase, every page and option, light and dark, iPhone 15 Pro Max, iOS 26.3.1, 2026-11-10; Dynamic Type sizes not checked on the phone |
| Foundation | ForgeAppKit application patterns (alerts and destinations as model values, load and submission state, form errors, Undo and progress banners, haptic feedback) | Device Lab 1.29915393, 2026-11-17, reported by Reuben: every banner and haptic on Device Lab's Interaction Patterns page, also with VoiceOver on (read out, Undo reachable), and Commonplace's Undo after Mark All as Read (the opened article stays read). FieldKit's Stop confirmation on top of the recording screen after #250 (2026-11-16, device-validation.md) |
| Foundation | Logging (platform log, recent records, extra sinks, panics logged) | Warn reached the USB log on iPhone (iOS 26.3.1, 2026-11-07); Info did not (LOG_INFO dropped by the relay), fixed to notice priority, recheck pending. Recent records: Device Lab's Tools > Logs showed records (Device Lab 1.29915595, 2026-11-17 evening, reported by Reuben); filter, search and Share not separately confirmed |
| Foundation | `forge.toml` app config (Info.plist, entitlements, xtool.yml) | builds only |
| Data and files | Relational store with queries (SQLite) | Chat's store (`forge-chat.db`) inside Relay (Device Lab 1.29915393, 2026-11-17, reported by Reuben): the existing account and conversations were still there after Chat moved into Relay, and sending and search worked. FieldKit's trip store (FieldKit in Device Lab 1.29913703 and 1.29913748, 2026-11-16, recorded by Reuben in device-validation.md): a trip recorded, renamed and deleted. The "Storage: SQLite" check in Settings was never done |
| Data and files | Full-text search | Chat search inside Relay passed (Device Lab 1.29915393, 2026-11-17, reported by Reuben: searched for a word in Chats); Commonplace article search was part of a general report (Device Lab 1.29913703, 2026-11-16: "Everything seems to be working", not item by item) |
| Data and files | File-backed upload and download with progress | a large document sent in Relay's Chats, with no freeze and the attachment shown (Device Lab 1.29915393, 2026-11-17, reported by Reuben). Downloading on another device not reported |
| Data and files | Background transfers | Device Lab on iPhone 15 Pro Max, iOS 26.3.1 (2026-11-11, 494faf1, reported by a person): downloads delivered after the app was ended (tests 1 and 3) and after leaving it running (test 2, completion callback not seen in the log), force quit marked Interrupted (test 6), once only on the second launch; Retry (then only a swipe action) was not found, so retry after force quit not yet seen |
| Data and files | Report export (zip, GPX, GeoJSON, PDF, PNG) | FieldKit in Device Lab 1.29913703 and 1.29913748, 2026-11-16, recorded by Reuben in device-validation.md: GPX and GeoJSON export "all worked" (a 16-point track) |
| Networking | Requests through URLSession (system proxy, VPN, HTTP/3, Low Data Mode) | Commonplace refresh through URLSession, and airplane mode reported as offline, iPhone 15 Pro Max, iOS 26.3.1, 2026-11-11; Soundscape Listen loading songs from its take server over Tailscale in Device Lab (after the ATS exception), reported working by a person, iPhone 15 Pro Max, iOS 26.3.1, 2026-11-16; Chat's stream through URLSession not yet run on a phone |
| Networking | Peer-to-peer transfer with resume | Relay in Device Lab on Cati (iOS 26.3.1), 2026-11-15, reported by Reuben: a 200 MB file sent from Linux with a word code arrived at the first try, every chunk checked (BLAKE3), 15.9 MB/s over Wi-Fi (research/relay-stage0-spike.md). iPhone to Linux with a word code also passed on 2026-11-15. Then on Device Lab 1.29915393, 2026-11-17, reported by Reuben: a folder from Linux arrived and showed in the Files app and the history; photos went iPhone to Linux as one folder; a 1 GB file resumed after Relay was swiped away ("Resuming: 510.9 MB already here") and finished, peak memory 74.6 MB. The QR-paired path not yet run on a phone. Trusted devices passed on 2026-11-17 (Device Lab build 1.29915595, reported by Reuben): after a code transfer the laptop trusted the iPhone, then a note and a file went laptop to iPhone with no code. Phone to laptop with no code passed the same evening (two notes received by `relay listen`); revoking not yet run on a phone |
| Native services and permissions | Local notifications | a Chat message notification from the other side arrived, Relay's Chats (Device Lab 1.29915393, 2026-11-17, reported by Reuben). Commonplace's and Good Dog's notifications not yet run on a phone |
| Native services and permissions | Photos picker | Relay's Send > Choose > Photos, two or three photos sent iPhone to Linux (Device Lab 1.29915393, 2026-11-17, reported by Reuben). Soundscape's artwork picker not yet reported |
| Native services and permissions | Haptics | Relay's tap when a trusted device's file arrives and when a message arrives in another chat were felt (Device Lab 1.29915595, 2026-11-17 evening, reported by Reuben, #309) |
| Media, sensors and rendering | Audio session handling | playback heard on an iPhone (Pocket Library read-aloud with a Premium voice, reported by a person, 2026-11-08); recording a voice message in Relay's Chats (Device Lab 1.29915393, 2026-11-17, reported by Reuben). Soundscape's recording and interruptions passed the same day, but Soundscape sets its own session, so they don't count here |
| Media, sensors and rendering | Record and play audio files | a voice message recorded and sent in Relay's Chats (Device Lab 1.29915393, 2026-11-17, reported by Reuben); playing it back was not reported |
| Media, sensors and rendering | Background audio | Soundscape Listen in Device Lab (playing with the screen locked, lock-screen controls, A/B comparison) reported working by a person, iPhone 15 Pro Max, iOS 26.3.1, 2026-11-16; one app only. Then Device Lab 1.29915393, 2026-11-17, reported by Reuben: the next song started by itself with the screen locked; and Device Lab 1.29915595, 2026-11-17 evening, reported by Reuben: the lock screen showed the title, artwork, play time and a progress bar, and another app's audio switched Soundscape to Play, which then continued (#306). Pocket Library's Read Aloud showed on the lock screen without details (fixed in #313, recheck pending) |
| Media, sensors and rendering | Location fix | Weather's own provider, 2026-11-06; the service is not yet run on an iPhone |
| Media, sensors and rendering | Continuous location and tracks | FieldKit in Device Lab 1.29913703 and 1.29913748, 2026-11-16, recorded by Reuben in device-validation.md: a trip recorded and stopped, its 16-point track exported. Recording with the phone locked not yet reported |
| Media, sensors and rendering | Device status (battery, Low Power Mode, thermal state, storage, memory, model) | Device Lab's This iPhone battery drain (Device Lab 1.29915595, 2026-11-17 evening, reported by Reuben); thermal state reported normal during the AI Lab runs the same day. Low Power Mode, storage and memory not separately reported |
| Media, sensors and rendering | Charts (Swift Charts) | FieldKit in Device Lab 1.29913703 and 1.29913748, 2026-11-16, recorded by Reuben in device-validation.md: the elevation and pace charts "look pretty good" (FK-2); one app |
| Extensions and restricted frameworks | Apple Foundation Models (on-device text generation) | Chat L1 on iPhone 15 Pro Max, iOS 26.3.1, free team (2026-11-08, ada3d23): streaming (first words 0.6-1.4 s), Stop, interrupted answers and airplane mode passed; characters wrote both sides and then refused (fixed in #104, rechecked on ba45825: in character, no refusals); characters now write requested content in their own voice (#113, rechecked on 90905a0). AI Lab, same phone, 2026-11-17 (combined Device Lab build with #277): all six experiments finished; first text 0.40-1.87 s, about 40-75 tokens a second, the app's memory up by at most 10 MB, thermal state normal throughout; `@Generable` (built with xtool on Linux) and the no-macro schema both returned typed answers (3.70 s and 2.24 s). Stop part-way through Long Answer worked (Reuben, 2026-11-17); the unavailable state was not exercised |
| Forge Device | Why an installed app doesn't work, for a person: the last run, the phone, the install and its signing, the newest crash and the app's log, with one next step (`forge diagnose`) | Cati, iOS 26.3.1, main e7ddafc, 2026-11-10, in the sitting with Reuben: Good Dog installed and healthy (installed, 7.0 days of signing, no crashes, 59 log lines, none errors) and the phone unplugged (No iPhone found, exit 1) both reported correctly. A crash at launch not yet run: no free-team slot and the weekly App ID limit reached |
| Forge UI Testing | Keep one WebDriverAgent runner between commands (`forge ui start`, `stop`, `status`) | start, reuse, status, stop and recovery after kill -9 all worked on iPhone16,2, iOS 26.3.1, 2026-11-08, run over Remote Control by the iPhone session; not yet confirmed by a person |
| Forge UI Testing | Find controls that have no identifier or label: by type with a label or index (`kind`, `index`), or by the text beside them (`near`) | iPhone16,2, iOS 26.3.1, 2026-11-08: a local patch with `kind = "TextView"` typed into DeepSeek and its Send (label "Send") was approved at the prompt. `near`, `index` and the unlabelled-tap gate in this form have not run on a phone |
| Forge UI Testing | Visual regression: screenshots compared with approved baselines, changed areas named by element (`forge ui run --visual`, `forge ui accept`) | Pocket Library m0 on iPhone16,2, iOS 26.3.1, 2026-11-08: record, then two unchanged runs matched (one cursor-sized area ignored). Run by the laptop session, not yet confirmed by a person; a real change (#116's label and identifier changes on the book form, 0.70% of pixels) was reported with the right elements on Cati the same day |

## Built, not yet run on an iPhone

The code exists and is tested on Linux, but nobody has recorded it working on a phone. Expect it to work, and expect surprises.

- **Foundation:** Rust application foundations: saved state as one versioned document (change-a-copy saves, newer files never touched, unreadable files set aside only on request) and field-by-field validation; Shared app shell (`app_shell!`: phase enum, `Cancellation`, event stream object); Lifecycle phases reported to Rust; Background work and cancellation in Rust; Finishing work in the background: commands and `finishing` work get the time iOS grants after the app leaves the screen, are interrupted when it runs out, and an unfinished run is reported at the next launch; Localize an app (String Catalogs built from Linux)
- **Data and files:** Backup and restore archive; App directories; Document picker import (copy into app); Share sheet and Save to Files; Managed file store; CSV import and export; Native reader (ForgeReader): documents as sections of styled blocks, HTML into blocks, positions as block anchors, the SwiftUI reading view
- **Offline delivery:** Durable outgoing queue (sends, edits, deletes, reactions, uploads); Retry with exponential backoff and jitter; Deduplication of retried sends; Delivery state reconciliation after reconnect
- **Networking:** Cancellable requests; Connectivity (online, Wi-Fi or cellular, metered, Low Data Mode, changes); Bonjour advertise and browse; Device identity and encrypted channel
- **Native services and permissions:** Keychain secrets; Notification actions and grouping; Deep links (custom URL schemes); Web pages inside the app (WebKit web view, Safari view, HTML the app has, bundled help pages); Permission interface; Location, motion and Bluetooth permissions; Microphone, camera, local network permissions; Barcode and QR scanning; Face ID app lock; Background refresh and processing tasks
- **Media, sensors and rendering:** Waveform summaries; Background location; Heading, altitude, motion; Bluetooth on or off; Forms with Rust validation; Apple Pencil (PencilKit)
- **Extensions and restricted frameworks:** HealthKit
- **Forge Device:** Installed apps, profile expiry and free-team slots (`forge device apps`); The app's log and crash reports collected during a UI run (`forge ui run`); Which Apple team xtool signs with, free or paid (`forge doctor`, `forge run ios`); Over-the-air installs of development builds over Tailscale: one Install page with every build by branch and commit (`forge publish ios`)
- **Forge UI Testing:** Swipe, long press, double tap, drag and keyboard keys on an element, scroll a lazy list until a row appears, and one-step `forge ui tap`, `type`, `scroll`, `swipe`, `long-press`, `double-tap`, `drag`, `key` and `expect` commands; Home, hardware buttons, open URL, system alerts and permission prompts, back, app state, permission reset, orientation, lock, settle, pickers, pinch, rotate, two-finger tap, tree filters, expect visible, hittable, selected and count, and terminal-only coordinate taps and swipes; Appearance and accessibility checks: dark or light mode at launch (`-ForgeAppearance`, `forge ui launch --appearance`), baselines per variant (`-dark`, `-ax5`), the audit of small tap targets and text cut off (`forge ui audit`); A person's acceptance: `forge verify --accept <ID> --note` records it as `kind = "human"`; `forge verify` shows twin, iPhone and person columns. (Run records moved to the forge-lab side project.)

## Not built yet

Planned only. An app can still use the Apple framework directly from Swift.

- **Offline delivery:** Interrupted upload recovery; Edit and delete conflict handling; Incoming alerts while suspended or terminated
- **Networking:** Multicast discovery
- **Native services and permissions:** Camera capture; Recurring schedules
- **Media, sensors and rendering:** Real-time audio processing in Rust; Maps (MapKit); Custom drawing and gestures; Metal rendering
- **Extensions and restricted frameworks:** Widgets; Share extension; Live Activities; ARKit; NFC

## Modules

Each module is a reusable part of the framework. **Stable** means its main capabilities work on an iPhone and are documented; **Experimental** means an app uses it but it isn't proven on the phone; **Planned** means it exists only in a plan or an open pull request.

| Module | Status | On iOS |
|---|---|---|
| Runtime | Experimental | Yes |
| Logging | Experimental | Partial: Swift writes through `ForgeLog` (`log_export!`) in every app (`swift-logging` parity rule); Device Lab's Logs showed recent records on the phone (2026-11-17); Info level on the USB log recheck pending |
| Persistence | Experimental | Yes |
| Managed files | Experimental | Partial: Chat attachments use it; device run pending |
| Networking | Experimental | Yes |
| Native services | Experimental | Partial: services check app only for most |
| Device services | Experimental | Partial: compiled in CI, not yet run on an iPhone |
| Media | Experimental | Partial: Relay (chat attachments) and Pocket Library use it; web pictures (Pocket Library covers, Soundscape artwork, Commonplace article images) loaded, didn't blink when scrolled back, and showed offline once seen (Device Lab 1.29915595, 2026-11-17, reported by Reuben, #298) |
| Developer tools | Experimental | Partial: host tools proven |
| Local AI | In progress: A0 shared by Chat and Pocket Library; A1 Relay asks built and tested on Linux | Partial: L0 check app ran on an iPhone 15 Pro Max (2026-11-07): Foundation Models available on a free team; Qwen3.5 0.8B at ~36 tok/s in release; stopping, offline and the 2B skipped, not measured. L1 (Chat's Assistant on Foundation Models) partly passed on the phone on 2026-11-08, and Device Lab's AI Lab passed on 2026-11-17 (see Apple Foundation Models) |
| Attachments | Planned | Partial: Relay's Chats (Chat M3, #56) sent a large document on the phone on 2026-11-17 |
| Documents | Planned | Partial: Pocket Library (#53) |
| Reader | Experimental | Partial: Pocket Library and Commonplace; device run pending |
| Audio | Experimental | Partial: compiled and tested in CI (macOS), not yet run on an iPhone |
| ForgeUI | Experimental | Yes: showcase built with xtool on Linux and run on an iPhone (light and dark, both themes), 2026-11-10 |
| ForgeAppKit | Experimental | Partial: built with xtool on Linux inside Device Lab (Pocket Library, Commonplace, FieldKit, Relay, Sketchbook, Soundscape); not yet recorded on an iPhone |
