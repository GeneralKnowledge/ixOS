# Status: what works today

<!-- Generated from docs/ledger/ by `forge ledger generate`. Edit the records there, not this page. -->

This page answers "does it work yet?" in one place. It is made from the same records as the [capability ledger](capability-ledger.md), which has every detail, and it follows the ledger's rule: something works on an iPhone only when a person ran an app that uses it on a physical iPhone and recorded the result. Built and tested on Linux is not enough. If another page's status note disagrees with this page, this page is the current one.

The newcomer path (set up Linux, `forge init`, `forge run ios`) is described in [getting-started.md](getting-started.md); the *Forge Device* and *Forge UI Testing* rows below say how far the tools for running and testing on a phone have been proven. Publishing to the App Store is not supported yet ([first-app-store-release.md](first-app-store-release.md)).

**Of 98 capabilities, 14 work on an iPhone, 10 partly work, 49 are built but haven't been run on an iPhone, and 25 aren't built yet.**

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
| Foundation | Logging (platform log, recent records, extra sinks, panics logged) | Warn reached the USB log on iPhone (iOS 26.3.1, 2026-11-07); Info did not (LOG_INFO dropped by the relay), fixed to notice priority, recheck pending |
| Foundation | `forge.toml` app config (Info.plist, entitlements, xtool.yml) | builds only |
| Media, sensors and rendering | Audio session handling | playback heard on an iPhone (Pocket Library read-aloud with a Premium voice, reported by a person, 2026-11-08); recording not yet run |
| Media, sensors and rendering | Location fix | Weather's own provider, 2026-11-06; the service is not yet run on an iPhone |
| Extensions and restricted frameworks | Apple Foundation Models (on-device text generation) | Chat L1 on iPhone 15 Pro Max, iOS 26.3.1, free team (2026-11-08, ada3d23): streaming (first words 0.6-1.4 s), Stop, interrupted answers and airplane mode passed; characters wrote both sides and then refused (fixed in #104, rechecked on ba45825: in character, no refusals); characters now write requested content in their own voice (#113, rechecked on 90905a0) |
| Forge UI Testing | Keep one WebDriverAgent runner between commands (`forge ui start`, `stop`, `status`; `forge mcp` holds one) | start, reuse, status, stop and recovery after kill -9 all worked on iPhone16,2, iOS 26.3.1, 2026-11-08, run over Remote Control by the iPhone session; not yet confirmed by a person |
| Forge UI Testing | Find controls that have no identifier or label: by type with a label or index (`kind`, `index`), or by the text beside them (`near`) | iPhone16,2, iOS 26.3.1, 2026-11-08: a local patch with `kind = "TextView"` typed into DeepSeek and its Send (label "Send") was approved at the prompt. `near`, `index` and the unlabelled-tap gate in this form have not run on a phone |
| Forge UI Testing | Visual regression: screenshots compared with approved baselines, changed areas named by element (`forge ui run --visual`, `forge ui accept`) | Pocket Library m0 on iPhone16,2, iOS 26.3.1, 2026-11-08: record, then two unchanged runs matched (one cursor-sized area ignored). Run by the laptop session, not yet confirmed by a person; a real change (#116's label and identifier changes on the book form, 0.70% of pixels) was reported with the right elements on Cati the same day |
| Forge UI Testing | Exploratory UI testing: walk an app's screens without a script and report crashes, error alerts, dead ends and accessibility problems with replay files (`forge ui explore`) | walked Pocket Library and found real accessibility issues, iPhone16,2, iOS 26.3.1, 2026-11-08, but that build changed the real library; the test-data version has not run on the phone yet |

## Built, not yet run on an iPhone

The code exists and is tested on Linux, but nobody has recorded it working on a phone. Expect it to work, and expect surprises.

- **Foundation:** ForgeUI design system (tokens, light/dark themes, styles and components on SwiftUI controls); ForgeAppKit application patterns (alerts and destinations as model values, load and submission state, form errors); Shared app shell (`app_shell!`: phase enum, `Cancellation`, event stream object); Lifecycle phases reported to Rust; Background work and cancellation in Rust
- **Data and files:** Relational store with queries (SQLite); Full-text search; App directories; Document picker import (copy into app); Share sheet and Save to Files; Managed file store; File-backed upload and download with progress
- **Offline delivery:** Durable outgoing queue (sends, edits, deletes, reactions, uploads); Retry with exponential backoff and jitter; Deduplication of retried sends; Delivery state reconciliation after reconnect
- **Networking:** Cancellable requests; Connectivity (online, Wi-Fi or cellular, metered, Low Data Mode, changes); Device identity and encrypted channel
- **Native services and permissions:** Keychain secrets; Local notifications; Notification actions and grouping; Deep links (custom URL schemes); Permission interface; Location, motion and Bluetooth permissions; Microphone, camera, local network permissions; Photos picker; Background refresh and processing tasks; Haptics
- **Media, sensors and rendering:** Record and play audio files; Waveform summaries; Background audio; Continuous location and tracks; Background location; Heading, altitude, motion; Device status (battery, Low Power Mode, thermal state, storage, memory, model); Bluetooth on or off; Forms with Rust validation
- **Extensions and restricted frameworks:** HealthKit
- **Forge Device:** Installed apps, profile expiry and free-team slots (`forge device apps`, in `forge_device_status`); The app's log and crash reports collected during a UI run or walk, and correlated with the failing step (`forge ui run`, `forge ui explore`, `forge_diagnose`); Why an installed app doesn't work, for a person: the last run, the phone, the install and its signing, the newest crash and the app's log, with one next step (`forge diagnose`); Live connection to a development build: pairing over USB, encrypted session (`forge device pair`, `unpair`); App log and lifecycle stream that survives the background (`forge device connect`); Status, state probes and app commands (`forge device status`, `snapshot`, `cmd`); Which Apple team xtool signs with, free or paid (`forge doctor`, `forge run ios`)
- **Forge UI Testing:** Appearance and accessibility checks: dark or light mode at launch (`-ForgeAppearance`, `forge_launch_app appearance`, `forge ui explore --appearance`), baselines per variant (`-dark`, `-ax5`), the audit of small tap targets and text cut off (`forge ui audit`, `forge_ui_audit`), and walks that say which screens look different from the last clean walk; Run record and a person's acceptance: `forge verify --run <start>` (and `forge_verify run: true`) writes one `kind = "run"` bundle and `report.md` with the commits, builds, checks, crashes, failures and their repairs, and every check not run with its reason; `forge verify --accept <ID> --note` records a person's acceptance as `kind = "human"`; `forge verify` shows twin, iPhone and person columns; Exploration aimed at a change: which screens and controls changed files touch, read from the source (`forge ui focus`), and walks that try them first, follow earlier walks' routes, replay their findings and compare with the previous walk (`forge ui explore --since/--changed`)

## Not built yet

Planned only. An app can still use the Apple framework directly from Swift.

- **Data and files:** Backup and restore archive; Background transfers; CSV import and export; Report export (zip, GPX, GeoJSON, PDF, PNG)
- **Offline delivery:** Interrupted upload recovery; Edit and delete conflict handling; Incoming alerts while suspended or terminated
- **Networking:** Bonjour advertise and browse; Peer-to-peer transfer with resume; Multicast discovery
- **Native services and permissions:** Camera capture; Barcode and QR scanning; Face ID app lock; Recurring schedules
- **Media, sensors and rendering:** Real-time audio processing in Rust; Maps (MapKit); Charts (Swift Charts); Custom drawing and gestures; Apple Pencil (PencilKit); Metal rendering
- **Extensions and restricted frameworks:** Widgets; Share extension; Live Activities; ARKit; NFC

## Modules

Each module is a reusable part of the framework. **Stable** means its main capabilities work on an iPhone and are documented; **Experimental** means an app uses it but it isn't proven on the phone; **Planned** means it exists only in a plan or an open pull request.

| Module | Status | On iOS |
|---|---|---|
| Runtime | Experimental | Yes |
| Logging | Experimental | Partial: Swift writes through `ForgeLog` (`log_export!`) in the starter and Chat; device run pending |
| Persistence | Experimental | Yes |
| Managed files | Experimental | Partial: Chat attachments use it; device run pending |
| Networking | Experimental | Yes |
| Native services | Experimental | Partial: services check app only for most |
| Device services | Experimental | Partial: compiled in CI, not yet run on an iPhone |
| Media | Experimental | Partial: Chat and Pocket Library use it; device run pending |
| Developer tools | Experimental | Partial: host tools proven, live session in #57 |
| Local AI | In progress: L1 in the Chat crate | Partial: L0 check app ran on an iPhone 15 Pro Max (2026-11-07): Foundation Models available on a free team; Qwen3.5 0.8B at ~36 tok/s in release; stopping, offline and the 2B skipped, not measured. L1 (Chat's Assistant on Foundation Models) built, not yet run on the phone |
| Attachments | Planned | Partial: Chat M3 (#56) |
| Documents | Planned | Partial: Pocket Library (#53) |
| Audio | Experimental | Partial: compiled and tested in CI (macOS), not yet run on an iPhone |
| ForgeUI | Experimental | Partial: compiled in CI (macOS), not yet built with xtool or run on an iPhone |
| ForgeAppKit | Experimental | Partial: compiled in CI (macOS), not yet built with xtool or run on an iPhone |
