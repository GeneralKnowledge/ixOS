# Capability ledger

<!-- Generated from docs/ledger/ by `forge ledger generate`. Edit the records there, not this page. -->

This page records what ixOS can actually do on an iPhone, one row per capability. It exists to stop us confusing "there is an API" with "it works". A capability is only proven when a real app has used it on a physical iPhone and a person recorded the result.

Draft, 2026-11-07. The plan behind it, with the demo-by-demo map, xtool limits and workstreams, is [ixOS capability plan: what the demos need](https://claude.ai/artifact/QA6NRXYy9SVSNfEG7LXZLx). Update the ledger in the same PR as any change to a capability.

Each table row is one record: a small TOML file under [`docs/ledger/`](ledger/), named after the row, with an `order` number that sets its position. To change a row, edit its file; to add one, add a file. Then run `forge ledger generate` (or `cargo run -p forge-cli -- ledger generate`), which rewrites this page; `cargo test` fails while the page and the records disagree. `forge ledger list <text>` finds a record.

## Columns

| Column | Means |
|---|---|
| Framework | A reusable piece exists in a Forge crate, template or `forge-services`, not only in one example app |
| Rust API | Rust code can use it, through UniFFI or a `forge-services` trait. `n/a` when it is deliberately native-only |
| iOS native | The Swift side exists (an adapter, a helper view, or documented direct use of an Apple framework) |
| In an app | An example or demo app uses it for a real feature |
| iPhone | Someone ran that app on a physical iPhone and recorded the result. Only results reported by a person count |
| Documented | A page in `docs/` explains it for third-party developers. Becomes **Yes** only once the developer experience exercise (below) has used that page |
| Limits | What does not work, or needs a paid team, a Mac, or a spike |

"Services check app" is the throwaway app `scripts/make-services-check-app.sh` builds for [native-services-device-check.md](native-services-device-check.md).

Values: **Yes**, **Partial** (with a note), **No**, **n/a**. Workstream ids (W1 to W11) and spike ids (S1 to S6) refer to the plan.

Android is work in progress ([roadmap](roadmap.md#android--work-in-progress)), so this ledger tracks iOS only. An Android column will be added when Android work resumes.

The core demo sequence is fixed: Chat, Pocket Ledger, FieldKit, Soundscape, Relay, Sketchbook. Each phase builds the smallest reusable capability its demo needs, proves it on the phone, and updates this page.

## Modules

This table is the capability registry: one row per framework module, saying what it provides, what it needs and how far it has got. The capability rows further down are the evidence for each module's status. The modules and their order come from the [framework extraction audit](framework-extraction-audit.md); the rules for what becomes a module are in [architecture.md](architecture.md#framework-principles).

Layers: **Core** (any app may need it), **Optional** (a substantial system an app opts into). Status: **Stable** (main capabilities proven on a physical iPhone and documented), **Experimental** (used by an app, not yet proven on the phone), **Planned** (only in a plan or an open PR). Platforms: **Yes**, **Partial**, **WIP** (Android: builds and tests, no new features), **Planned**, **No**. "Desktop" means a Linux, macOS or Windows app host; today Rust code runs on Linux only in tests and tools.

| Module | Layer | Crate and feature | Provides | Native APIs | Dependencies | iOS | Android | Desktop | Status | Exercised by |
|---|---|---|---|---|---|---|---|---|---|---|
| Runtime | Core | `forge-runtime` | Background work, cancellation, event streams, lifecycle, `app_shell!` | None | `log`; `forge-net` for the Android certificate init | Yes | WIP | Planned | Experimental | Every app |
| Logging | Core | `forge-runtime::logging`; `log_export!` and `ForgeLog.swift` for the host | One log pipeline for Rust and Swift, recent records, extra sinks, panics logged | System log (`syslog`), logcat | `log` | Partial: Swift writes through `ForgeLog` (`log_export!`) in the starter and Chat; device run pending | WIP | Planned | Experimental | Every app |
| Persistence | Core | `forge-storage`; SQLite under `sqlite` | Atomic writes, versioned JSON, SQLite with migrations, FTS5, recovery | None | `rusqlite` (bundled SQLite) with `sqlite` | Yes | WIP: JSON only | Planned | Experimental | Chat, Pocket Library, Weather, Counter, starter |
| Managed files | Core | `forge-storage::files` (`FileStore`); SHA-256 naming and verified downloads under `hash` | Copy-in, app or content naming, verified downloads, relative names only, size limit, orphan and crash sweep, usage. Backup exclusion not yet | None yet; backup exclusion will need `URLResourceValues` on the Swift side | `sha2` with `hash` | Partial: Chat attachments use it; device run pending | Partial: plain Rust, Chat Android builds against it | Planned | Experimental | Chat attachments; Pocket Library covers, books, reader images and exports; AI models next |
| Networking | Core | `forge-net`; HTTPS under `ureq` | HTTP requests, streams, cancellation, file upload and download (#56), `Backoff` (retry delays with jitter). Connectivity is in Device services | Platform certificate verifier | `ureq`, `rustls` | Yes | WIP | Planned | Experimental | Weather, Chat, Pocket Library |
| Native services | Core | `forge-services` (`native_services!` and `swift/`) | Keychain, app directories, file import, sharing, local notifications, permissions, deep links | Security, UserNotifications, SwiftUI | `uniffi`, `async-trait` | Partial: services check app only for most | No | No | Experimental | Chat (Keychain), services check app |
| Device services | Core, opt-in per app | `forge-services`: `native_services!(connectivity, device_status, motion, location, bluetooth)`; adapters in `swift/optional/`, copied only when selected | Connectivity and a Wi-Fi-only transfer policy; battery, power, thermal, storage, memory and the heavy-work rule; accelerometer, gyroscope, magnetometer, device motion, barometer; location and heading; Bluetooth state; location, motion and Bluetooth permissions | Network, UIKit, ProcessInfo, Core Motion, Core Location, Core Bluetooth | `uniffi`, `async-trait` | Partial: compiled in CI, not yet run on an iPhone | No | No | Experimental | Device Lab; meant for AI downloads (connectivity, heavy-work rule), Chat reconnect, FieldKit |
| Media | Core | `ForgeImages` (`forge-services/swift/helpers`, copied into every iOS app) | Thumbnails from a file or data, optional memory cache, downscaled metadata-free JPEG, all off the main thread | ImageIO | None | Partial: Chat and Pocket Library use it; device run pending | No | No | Experimental | Chat, Pocket Library |
| Developer tools | Optional | `forge-devtools` (app, `devtools` feature), `forge-devproto`, `forge-device` (host) | Phone details, logs and crashes from Linux; live paired session with log stream, probes, commands (#57) | usbmuxd on the host; nothing native in the app | `idevice`, `snow`, `ciborium` | Partial: host tools proven, live session in #57 | No | n/a | Experimental | Starter app, any app over USB |
| Local AI | Optional | `forge-ai` (planned, #55 L5) | Model library, download, inference behind a foreign trait, token streaming, cancellation, resource guard | Foundation Models, Core ML (CoreML-LLM) | Swift packages in the app | Partial: L0 check app ran on an iPhone 15 Pro Max (2026-11-07): Foundation Models available on a free team; Qwen3.5 0.8B at ~36 tok/s in release; stopping, offline and the 2B skipped, not measured. L1 (Chat's Assistant on Foundation Models) built, not yet run on the phone | No | No | In progress: L1 in the Chat crate | Chat Assistant (L1: conversations, characters, streaming, Stop, history; Foundation Models) |
| Attachments | Optional | chat-core today; generic parts to Managed files and Networking | Upload queue and recovery, on top of Managed files | Photos picker, Quick Look | Managed files, Networking | Partial: Chat M3 (#56) | No | No | Planned | Chat |
| Documents | Optional | Pocket Library today; `epub` feature later | EPUB metadata, chapters as styled text blocks, positions to pages | None | `zip`, `quick-xml` | Partial: Pocket Library (#53) | No | No | Planned | Pocket Library |
| Audio | Optional | `forge-services`: `native_services!(audio)`; adapter `swift/optional/ForgeServices+Audio.swift` (#60); `ForgeAudioSession` is a helper in every app (`swift/helpers/`) | Microphone permission, recording (`ForgeAudioRecorder`), one-at-a-time playback (`ForgeAudioPlayer`), audio session (`ForgeAudioSession`), waveforms and lengths; Now Playing, remote commands and speech synthesis later | AVFoundation, MediaPlayer | None | Partial: compiled and tested in CI (macOS), not yet run on an iPhone | No | No | Experimental | Chat M4 voice; Pocket Library read-aloud (session only; its lock-screen controls stay in the app until a second user) |

## Foundation

| Capability | Framework | Rust API | iOS native | In an app | iPhone | Documented | Limits |
|---|---|---|---|---|---|---|---|
| App-owned Rust crate with UniFFI async exports | Yes | Yes | Yes | Yes: starter, Pocket Library | No: built with xtool, install not recorded | Yes: app-crate.md | Every example owns its crate; the Android Chat example keeps a frozen copy of the old Chat API in its own crate |
| Native SwiftUI structure (navigation, lists, sheets, search) | Yes: starter | n/a | Yes | Yes: starter, Pocket Library | No | Yes: ios-ui.md | Older examples use `ObservableObject` |
| Shared app shell (`app_shell!`: phase enum, `Cancellation`, event stream object) | Yes: forge-runtime | Yes | n/a | Yes: starter, Chat, Pocket Library | No | Partial: app-crate.md | |
| Logging (platform log, recent records, extra sinks, panics logged) | Yes: forge-runtime | Yes: `forge_runtime::logging` | n/a: Rust writes the system log via `syslog` | Yes: starter and every example call `logging::init` | Partial: Warn reached the USB log on iPhone (iOS 26.3.1, 2026-11-07); Info did not (LOG_INFO dropped by the relay), fixed to notice priority, recheck pending | Partial: app-crate.md, architecture.md | Swift logs through `ForgeLog` (crates calling `forge_runtime::log_export!()`), so its records share the pipeline; no file sink yet |
| Lifecycle phases reported to Rust | Yes: forge-runtime | Yes | Yes: `.forgeLifecycle` | Yes: starter | No | Yes | |
| Background work and cancellation in Rust | Yes: forge-runtime | Yes | n/a | Yes | No | Yes | Stops when iOS suspends the app |
| `forge.toml` app config (Info.plist, entitlements, xtool.yml) | Yes | n/a | n/a | Yes | Partial: builds only | Yes: app-config.md | No asset catalogs on Linux |

## Data and files (W1, W2)

| Capability | Framework | Rust API | iOS native | In an app | iPhone | Documented | Limits |
|---|---|---|---|---|---|---|---|
| JSON snapshot storage with migration hook | Yes: forge-storage | Yes | n/a | Yes: Chat, starter | Yes: Chat, 2026-11-05 | Yes | Rewrites the whole file on every change |
| Relational store with queries (SQLite) | Yes: forge-storage `Database` (feature `sqlite`): migrations, transactions, recovery | Yes | n/a | Yes: Chat M2 (`forge-chat.db`) | No: Chat M2 device run skipped | Partial: chat-demo.md | Bundled SQLite builds with xtool (Chat M2); the phone run that would show "Storage: SQLite" in Settings has not been done |
| Full-text search | Partial: FTS5 is built into `Database`; the query builder is still Chat's own (`fts_query`) | Yes | n/a | Yes: Chat M2 search | No | Partial: chat-demo.md | |
| Backup and restore archive | No | No | No | No | No | No | |
| App directories | Yes: forge-services | Yes | Yes | Partial: services check app only | No | Yes | Store relative paths only |
| Document picker import (copy into app) | Yes | Partial: path handed to Rust | Yes: `forgeFileImporter` | Partial: services check app only | No | Yes | |
| Share sheet and Save to Files | Yes | Yes: `ShareItem` | Yes: `ForgeShareLink` | Partial: services check app only | No | Yes | |
| Managed file store | Yes: forge-storage `FileStore` (`files` module, `hash` feature for SHA-256) | Yes | n/a | Partial: Chat M3 attachments (`AttachmentStore` keeps Chat's limits on top); Pocket Library covers, EPUBs, reader images and exports | No | Partial: rustdoc, architecture.md | No backup exclusion yet |
| File-backed upload and download with progress | Partial: forge-net `HttpClient::send_file`, `download_to_file` | Yes | n/a | Partial: Chat M3 | No | No | Streams from and to files; no progress reporting yet |
| Background transfers | No | No | No | No | No | No | Needs native `URLSession` (S6) |
| CSV import and export | No | No | n/a | No | No | No | |
| Report export (zip, GPX, GeoJSON, PDF, PNG) | No | No | No | No | No | No | PDF and PNG drawn natively |

## Offline delivery (W11)

Chat's offline behaviour, tracked as its own capability because it tests the application architecture, not just networking.

| Capability | Framework | Rust API | iOS native | In an app | iPhone | Documented | Limits |
|---|---|---|---|---|---|---|---|
| Durable outgoing queue (sends, edits, deletes, reactions, uploads) | No: Chat only | Yes: chat-core outbox, stored before the network call | n/a | Yes: Chat M2 | No: Chat M2 device run skipped | Partial: chat-demo.md | Survives a relaunch in Rust tests; not yet checked across a kill on the phone |
| Retry with exponential backoff and jitter | Yes: forge-net `Backoff` (initial, cap, jitter, reset, cancellable wait) | Yes | n/a | Yes: Chat stream reconnect (with jitter) and outbox | No | Partial: rustdoc, chat-demo.md | |
| Deduplication of retried sends | No: Chat only | Yes: client-chosen ids in Chat | n/a | Yes: Chat | No | Yes: chat-demo.md | Needs the new server to keep honouring ids |
| Delivery state reconciliation after reconnect | No | Partial: `since` catch-up | n/a | Partial: Chat | No | Partial | No read state on the server yet |
| Interrupted upload recovery | No | No | No | No | No | No | Depends on W2 file-backed uploads |
| Edit and delete conflict handling | No | No | n/a | No | No | No | Server order wins |
| Incoming alerts while suspended or terminated | No | No | No | No | No | No | Needs APNs: paid team, and a non-goal since 2026-11-05. Local notifications and background tasks are not a substitute |

## Networking (W7)

| Capability | Framework | Rust API | iOS native | In an app | iPhone | Documented | Limits |
|---|---|---|---|---|---|---|---|
| HTTPS requests with typed errors | Yes: forge-net | Yes | n/a | Yes: Weather, Chat | Yes: 2026-11-05 and 2026-11-06 | Yes | |
| Long-lived streams (SSE) with reconnect | Yes | Yes | n/a | Yes: Chat | Yes: Chat | Yes: chat-demo.md | |
| Cancellable requests | Yes | Yes | Yes: task cancellation | Yes: starter | No | Yes | |
| Connectivity (online, Wi-Fi or cellular, metered, Low Data Mode, changes) | Yes: forge-services `connectivity` | Yes: `ConnectivityService`, `TransferPolicy::Unmetered` | Yes: `NWPathMonitor` | Yes: Device Lab | No | Partial: native-services.md | Wi-Fi network name needs the paid-team Access Wi-Fi Information entitlement; no signal strength or carrier |
| Bonjour advertise and browse | No | No | No | No | No | No | Needs `NSBonjourServices` key |
| Peer-to-peer transfer with resume | No | No | No | No | No | No | |
| Device identity and encrypted channel | Partial: Keychain only | Partial | Partial | No | No | No | Use a reviewed library |
| Multicast discovery | No | No | No | No | No | No | Paid team plus Apple approval |

## Native services and permissions (W3, W4)

| Capability | Framework | Rust API | iOS native | In an app | iPhone | Documented | Limits |
|---|---|---|---|---|---|---|---|
| Keychain secrets | Yes | Yes | Yes | Partial: services check app only | No | Yes | Synchronous; call off the UI thread |
| Local notifications | Yes | Yes | Yes | Yes: Chat messages (`LocalChatNotifier` on `ForgeNotifications`), services check app | No | Yes | No push: APNs is a non-goal |
| Notification actions and grouping | Partial: grouping (`thread_id`), subtitle, tap opens a deep link; no actions | Yes | Yes | Yes: Chat groups by room and opens the room on tap | No | Partial: native-services.md, chat-demo.md | No action buttons or categories yet |
| Deep links (custom URL schemes) | Yes | Yes | Yes | Partial: services check app only | No | Yes | Universal links need a paid team |
| Permission interface | Partial: notifications, location, motion, Bluetooth, microphone | Partial | Partial | Yes: Device Lab (location, motion, Bluetooth); Partial: Chat M4 (microphone) | No | Yes | A permission whose optional service the app did not select reports `Unsupported` |
| Location, motion and Bluetooth permissions | Yes: forge-services | Yes: `NativePermission` | Yes | Yes: Device Lab | No | Partial: native-services.md | Asking without the `forge.toml` usage string returns `Unsupported` instead of hanging or crashing |
| Microphone, camera, local network permissions | Partial: microphone (`audio` service) | Partial: `NativePermission::Microphone` | Partial: microphone | Partial: Chat M4 (microphone) | No | Partial: native-services.md | Camera and local network: config only |
| Photos picker | No | n/a | Partial: direct `PhotosPicker` use in Chat, shrunk with ImageIO | Partial: Chat M3 | No | No | No permission needed |
| Camera capture | No | No | No | No | No | No | |
| Barcode and QR scanning | No | No | No | No | No | No | VisionKit scanner |
| Face ID app lock | No | No | No | No | No | Partial: config only | |
| Background refresh and processing tasks | Partial: config key | No | No | No | No | Partial | iOS decides when they run |
| Recurring schedules | No | No | n/a | No | No | No | |
| Haptics | n/a | n/a | Yes: SwiftUI | No | No | No | |

## Media, sensors and rendering (W5, W6, W8)

| Capability | Framework | Rust API | iOS native | In an app | iPhone | Documented | Limits |
|---|---|---|---|---|---|---|---|
| Audio session handling | Yes: forge-services Swift helper, in every app (no permission needed) | n/a | Yes: `ForgeAudioSession` | Partial: Chat M4 voice, Pocket Library read-aloud | Partial: playback heard on an iPhone (Pocket Library read-aloud with a Premium voice, reported by a person, 2026-11-08); recording not yet run | Partial: native-services.md | Record and spoken playback only; no mixing or ducking options yet |
| Record and play audio files | Yes: forge-services `audio` (optional) | Yes: `AudioService::duration`, `NativePermission::Microphone` | Yes: `ForgeAudioRecorder`, `ForgeAudioPlayer`, waveforms | Partial: Chat M4 | No | Partial: native-services.md | AAC `.m4a`; copied only into apps that select `audio` |
| Waveform summaries | Yes: forge-services `audio` (optional) | n/a | Yes: `ForgeAudio.waveform(of:bars:)`, off the main actor | Partial: Chat M4 | No | Partial: native-services.md | Peaks per slice from the decoded file; no precomputed summaries sent with messages |
| Real-time audio processing in Rust | No | No | No | No | No | No | UniFFI unusable on the render thread; S4 |
| Background audio | Partial: config | No | No | No | No | Partial | |
| Location fix | Yes: forge-services `location` | Yes: `LocationService` | Yes: `CLLocationManager` | Yes: Device Lab; Weather still has its own one-shot provider | Partial: Weather's own provider, 2026-11-06; the service is not yet run on an iPhone | Partial: native-services.md | When in use only |
| Continuous location and tracks | Partial: continuous fixes and `distance_m`; no track storage | Yes | Yes | Partial: Device Lab distance travelled | No | Yes | No GPX or track file yet (FieldKit) |
| Background location | Partial: config | No | No | No | No | Partial | Needs "always" permission |
| Heading, altitude, motion | Yes: forge-services `motion` and `location` | Yes: `MotionService` (accelerometer, gyroscope, magnetometer, device motion, barometer) | Yes: Core Motion, Core Location heading | Yes: Device Lab | No | Partial: native-services.md | Foreground only; the barometer needs the Motion & Fitness permission; 10 ms to 10 s intervals |
| Device status (battery, Low Power Mode, thermal state, storage, memory, model) | Yes: forge-services `device_status` | Yes: `DeviceStatusService`, `defer_heavy_work` | Yes | Yes: Device Lab | No | Partial: native-services.md | Device name is the generic model since iOS 16 (the real name needs an Apple-granted entitlement) |
| Bluetooth on or off | Yes: forge-services `bluetooth` | Yes: `BluetoothService` | Yes: `CBCentralManager` | Yes: Device Lab | No | Partial: native-services.md | State only: no scanning or connections yet |
| Maps (MapKit) | n/a | n/a | No | No | No | No | MapKit has no offline tiles |
| Forms with Rust validation | Partial: compose sheet | Partial | Partial | Yes: starter | No | Partial | |
| Charts (Swift Charts) | No | n/a | No | No | No | No | |
| Custom drawing and gestures | No | n/a | No | No | No | No | |
| Apple Pencil (PencilKit) | No | n/a | No | No | No | No | |
| Metal rendering | No | No | No | No | No | No | Precompiled shaders need a Mac; S3 |

## Extensions and restricted frameworks (W9, W10)

| Capability | Framework | Rust API | iOS native | In an app | iPhone | Documented | Limits |
|---|---|---|---|---|---|---|---|
| Widgets | No | No | No | No | No | No | xtool supports extensions; each uses a free-team App ID (S2) |
| Share extension | No | No | No | No | No | No | |
| Live Activities | No | No | No | No | No | No | Needs a widget extension |
| HealthKit | Partial: entitlement in config | No | No | No | No | Partial | Free-team signing unproven (S5) |
| ARKit | No | n/a | No | No | No | No | Device only |
| Core ML | No | No | Partial: CoreML-LLM in the L0 check app | Partial: AI check app | Yes: Qwen3.5 0.8B (copied in over USB) compiled on the phone and run, iPhone 15 Pro Max, iOS 26.3.1, 2026-11-07 | Partial: local-ai.md | `.mlmodel` compiles only on a Mac, but downloaded `.mlpackage` models compile on the phone (`MLModel.compileModel`); needs free disk for the compiled copy |
| Apple Foundation Models (on-device text generation) | No: in the Chat app crate until L5 (`forge-ai`) | Yes: `LocalModelEngine` foreign trait and `TokenSink` (Chat, L1) | Yes: `FoundationModelsEngine` (Chat), streamed, stops when Rust says so | Yes: Chat's Assistant (L1) | Partial: Chat L1 on iPhone 15 Pro Max, iOS 26.3.1, free team (2026-11-08, ada3d23): streaming (first words 0.6-1.4 s), Stop, interrupted answers and airplane mode passed; characters wrote both sides and then refused (fixed in #104, rechecked on ba45825: in character, no refusals); characters now write requested content in their own voice (#113, rechecked on 90905a0) | Partial: local-ai.md, Chat ACCEPTANCE.md (L1) | iOS 26 with Apple Intelligence on; 4,096-token context; Apple's guardrails can refuse; plain text only (`@Generable` needs Xcode's macro plugin) |
| NFC | No | No | No | No | No | No | Paid team |

## Forge Device (W12)

Developer tooling on the Linux computer, not app capabilities, so "Rust API" and "iOS native" are n/a. "In an app" means used against an app on the phone.

| Capability | Framework | Rust API | iOS native | In an app | iPhone | Documented | Limits |
|---|---|---|---|---|---|---|---|
| Phone details from Linux (`forge device list`, `info`) | Yes: forge-device | n/a | n/a | n/a | Yes: iPhone16,2, iOS 26.3.1, 2026-11-07 | Partial: forge-device.md | USB only; Wi-Fi-synced phones are listed but untested |
| Live system log, filtered by app (`forge device logs`) | Yes: forge-device | n/a | n/a | Yes: any app; Rust logs via `forge_runtime::logging` | Yes: iPhone, iOS 26.3.1, 2026-11-07 (`--grep`, `--process`, `--limit`) | Partial: forge-device.md | Apps from `forge init` run as `<App>Xtool`; older apps (Pocket Library, Device Lab) still share `AppHostXtool`, so filter those on the log target; no OS trace (structured `os_log` metadata) yet |
| Crash reports (`forge device crashes`) | Yes: forge-device | n/a | n/a | Yes: any app | Yes: iPhone, iOS 26.3.1, 2026-11-07 (listed 161, pulled 4) | Partial: forge-device.md | Symbolication: see the forge-crash row |
| Symbolicated crash reports (`forge crash`, `forge device crashes --pull`, `forge_read_crashes`) | Yes: forge-crash | n/a | n/a | Partial: Linux tests on an LLVM-built arm64 iOS binary (dSYM and debug map) and hand-written iOS 26 reports | Partial: real iOS 26.3.1 reports from Cati parsed and classified (5 crashes of other apps: Swift runtime error, trap, watchdog), 2026-11-08; no Forge app crash symbolicated yet | Yes: forge-device.md | Needs the exact build that crashed on this computer (matched by UUID); Swift names need the Swift toolchain to demangle; Rust frames get file and line only with debug line tables in the release profile; .ips only (iOS 15+) |
| Live connection to a development build: pairing over USB, encrypted session (`forge device pair`, `unpair`) | Yes: forge-devproto, forge-devtools, forge-device | Yes: `forge_devtools::DevTools` | n/a: pure Rust in the app | Yes: starter app (`--devtools`) | No | Yes: forge-device.md | USB only; no QR or Wi-Fi pairing yet |
| App log and lifecycle stream that survives the background (`forge device connect`) | Yes: forge-devtools, forge-device | Yes | n/a | Yes: starter app | No | Yes: forge-device.md | 2,000-record buffer, older records reported as a gap; Swift-side logs not included |
| Status, state probes and app commands (`forge device status`, `snapshot`, `cmd`) | Yes: forge-devtools, forge-device | Yes: `DevTools::probe`, `DevTools::command` | n/a | Yes: starter app (`notes` probe, `log` command) | No | Yes: forge-device.md | Chat registers none yet; no built-in reset or test-notification commands |
| Which Apple team xtool signs with, free or paid (`forge doctor`, `forge run ios`) | Yes: forge-cli | n/a | n/a | n/a | No | Yes: ios-signing.md | Reads `xtool auth status` and `xtool ds teams list` (needs network to tell free from paid); paid detection follows xtool's own rule, never checked against a paid team yet. On Cati (2026-11-07) xtool signed with the free personal team |

## Forge UI Testing (W12)

Driving an app on the phone from Linux through XCTest and WebDriverAgent. Plan and feasibility: [forge-ui-testing.md](forge-ui-testing.md).

| Capability | Framework | Rust API | iOS native | In an app | iPhone | Documented | Limits |
|---|---|---|---|---|---|---|---|
| Mount the Developer Disk Image and start the WebDriverAgent runner from Linux (`forge ui setup`) | Yes: forge-uitest | n/a | n/a: prebuilt WDA, signed by xtool | n/a | Yes: iPhone16,2, iOS 26.3.1, WDA 16.15.2, 2026-11-07 (M0 run watched by Reuben) | Partial: forge-ui-testing.md | Runner uses one of the 3 free-account app slots and expires after 7 days; the personalised DDI comes from the doronz88/DeveloperDiskImage mirror (Xcode 26.0's xip has none) and is gone after a reboot. Needs Settings > Developer > Enable UI Automation on the phone (else WDA code 41) |
| Launch an app, find elements by accessibility identifier, tap, type, read the UI tree (`forge ui run`, `tree`) | Yes: forge-uitest | n/a | n/a | Yes: Pocket Library (`uitests/m0.toml`) | Yes: M0 passed 11/11 in 10 s, iPhone16,2, iOS 26.3.1, 2026-11-07 (watched by Reuben) | Partial: forge-ui-testing.md | No coordinates by design; no reports until M1. Without the runner, Apple's accessibility service (docs/research/ax-audit.md) reads identifiers and presses by identifier on iPhone16,2, iOS 26.3.1 (2026-11-07), but gives a flat list, can't type, and is an experimental probe, not `forge ui` |
| Act in apps Forge didn't build, under a per-app policy (`forge ui --any-app`, `~/.config/forge/device-control.toml`) | Yes: forge-uitest `policy` | n/a | n/a | Yes: the Claude app (com.anthropic.claude, App Store) at `operate`, and Pocket Library unchanged | Yes: iPhone16,2, iOS 26.3.1, 2026-11-08 (Reuben present): Pocket Library m0 11/11 without --any-app; Claude app typed, then Send stopped at the [y/N] prompt and was refused on n (journal: refused, presses_return false); refused with no terminal; refused without --any-app before any action. ax_probe Ctrl-C exited cleanly, but before the walk started, so cleanup mid-walk is unproven | Partial: forge-ui-testing.md (Apps Forge didn't build), research/general-device-control.md | Commit gate matches words in labels and identifiers, so it can miss commits; approval only at a terminal (MCP and scripts are refused); MCP has no --any-app yet; system apps (com.apple.*) aren't resolved until the app inventory lands; the hard-refusal list is by bundle id and unchecked on a phone |
| Keep one WebDriverAgent runner between commands (`forge ui start`, `stop`, `status`; `forge mcp` holds one) | Yes: forge-uitest (session) | n/a | n/a | No | Partial: start, reuse, status, stop and recovery after kill -9 all worked on iPhone16,2, iOS 26.3.1, 2026-11-08, run over Remote Control by the iPhone session; not yet confirmed by a person | Yes: forge-ui-testing.md | Measured on iPhone16,2 (2026-11-08): `forge ui tree` 0.5 s with a session against 1.4 to 1.9 s without; `ui start` 1.5 s; a whole test file saves only one runner start (m0 9.2 s against 10.1 s), so the gain is per command, not per step. `forge mcp` holding one is not yet tried on the phone. Restarts the runner after two missed `/status` checks, gives up after three failed restarts. One session per phone; stops after 30 idle minutes by default |
| Find controls that have no identifier or label: by type with a label or index (`kind`, `index`), or by the text beside them (`near`) | Yes: forge-uitest `Locator::Kind`, `Locator::Near`; test files and `forge_ui_act` | n/a | n/a | Partial: DeepSeek's composer (com.deepseek.chat.nov, App Store) through a local patch with `kind = "TextView"` only | Partial: iPhone16,2, iOS 26.3.1, 2026-11-08: a local patch with `kind = "TextView"` typed into DeepSeek and its Send (label "Send") was approved at the prompt. `near`, `index` and the unlabelled-tap gate in this form have not run on a phone | Yes: forge-ui-testing.md (Targets, Apps Forge didn't build) | `index` counts in tree order across the whole screen, so the keyboard or a new row can shift it; `near` needs element frames and reads the tree before each search (one extra round trip); a tap on a control with no label or identifier in another app always asks; no `ref` handles yet |
| Screenshot saved on Linux (`forge ui screenshot`) | Yes: forge-uitest | n/a | n/a | Yes: Pocket Library | Yes: 1290x2796 PNGs from M0, iPhone16,2, iOS 26.3.1, 2026-11-07 | Partial: forge-ui-testing.md | Comparison with approved baselines is a separate record (visual regression) |
| Visual regression: screenshots compared with approved baselines, changed areas named by element (`forge ui run --visual`, `forge ui accept`) | Yes: forge-uitest (visual, baseline) | n/a | n/a | No: no app has baselines yet | Partial: Pocket Library m0 on iPhone16,2, iOS 26.3.1, 2026-11-08: record, then two unchanged runs matched (one cursor-sized area ignored). Run by the laptop session, not yet confirmed by a person; a deliberate change not yet tried | Yes: visual-regression.md | Exact pixels with a per-channel tolerance and a minimum area size, not perceptual; status bar and named elements masked; saving the tree with each screenshot adds about 2.5 s per screenshot step; baselines are per screen size and must come from the same build kind |
| Accessibility identifiers and labels in the demos | No | n/a | Partial: labels per ios-ui.md | Partial: Pocket Library's library screen and book form | Yes: Pocket Library ids (library.*, bookForm.*, tab.*) found through WDA, iPhone16,2, iOS 26.3.1, 2026-11-07 | Partial: ios-ui.md | Conventions and the full audit come in M3 |
| Exploratory UI testing: walk an app's screens without a script and report crashes, error alerts, dead ends and accessibility problems with replay files (`forge ui explore`) | Yes: forge-uitest (`explore`) | n/a | n/a | Partial: Pocket Library (test-data mode `-ForgeUITestData`) | Partial: walked Pocket Library and found real accessibility issues, iPhone16,2, iOS 26.3.1, 2026-11-08, but that build changed the real library; the test-data version has not run on the phone yet | Yes: forge-explore.md | Needs the app's `[ui_test] test_data_args` to change anything; `--real-data` only looks. Forge apps only (not yet wired to the any-app policy); no scrolling below the fold; one row per list; no invariants, performance or crash-report checks during the walk; no MCP tool yet (agents use `forge --json ui explore`) |

## Performance

A capability can work and still give a poor app. Each metric has a proposed target, a way to measure it from Linux with xtool (timestamps the app logs, read from the device log, or a diagnostics screen in the app), and the last recorded figure. Targets are proposals until the first measurement on the test iPhone, then they are revised once and kept.

| Metric | Proposed target | How measured | First needed by | Last recorded |
|---|---|---|---|---|
| Cold launch to first interactive screen (release build) | Under 1 s | Process start and first-frame timestamps logged by the app | Chat M1 | Not measured |
| Rust-to-Swift call overhead | Under 50 µs median for a sync call, under 1 ms for an async round trip | Benchmark screen in the services check app | Chat M0 | Not measured |
| Large-list scrolling | No hitches scrolling 10,000 rows (messages, transactions) | Frame-hitch counter from `CADisplayLink` in a debug overlay | Chat M1, Pocket Ledger | Not measured |
| Database queries | Full-text search over 100,000 messages under 50 ms; a month's budget totals under 20 ms | Timed in Rust and logged | Chat M2, Pocket Ledger | Not measured |
| Attachment import and thumbnail | A 12 MP photo imported and thumbnailed in under 500 ms, never decoded on the main thread | Timed in the app | Chat M3 | Not measured |
| Memory | Chat with 10,000 messages under 150 MB; a widget extension under 30 MB | Footprint logged by the app | Chat M3, W9 | Not measured |
| Audio glitches and latency | No underruns in 10 minutes; under 20 ms round trip at a 256-frame buffer | Underrun counter in the render callback; the audio session's buffer duration | Spike S4, Soundscape | Not measured |

## Developer experience: build without the maintainer

The day-to-day loop (create, change, build, launch, inspect, diagnose) is assessed in [dev-loop.md](dev-loop.md), with measured times and the gaps that block it.

The demos prove that we can build apps with ixOS, not that someone else can. In this exercise a developer who has not worked on ixOS gets a clean Ubuntu machine, the docs and an iPhone, and nothing else. It runs first after Chat M6 and again after the sixth demo.

| Step | Docs they start from | Last run | Time taken | Findings |
|---|---|---|---|---|
| Install the toolchain on Ubuntu | linux-setup.md, getting-started.md | Not run | | |
| Create a new project | getting-started.md | Not run | | |
| Add a native iOS service | native-services.md | Not run | | |
| Implement Rust application logic | app-crate.md | Not run | | |
| Build and install on a physical iPhone | builds.md, ios-signing.md | Not run | | |
| Diagnose a configuration error we introduce on purpose | troubleshooting.md | Not run | | |

Also recorded for each run: time to the first successful iPhone build, every time they had to read framework source, missing or wrong documentation, confusing APIs, and any platform workaround they needed.
