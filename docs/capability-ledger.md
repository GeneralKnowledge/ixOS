# Capability ledger

This page records what ixOS can actually do on an iPhone, one row per capability. It exists to stop us confusing "there is an API" with "it works". A capability is only proven when a real app has used it on a physical iPhone and a person recorded the result.

Draft, 2026-11-07. The plan behind it, with the demo-by-demo map, xtool limits and workstreams, is [ixOS capability plan: what the demos need](https://claude.ai/artifact/QA6NRXYy9SVSNfEG7LXZLx). Update this page in the same PR as any change to a capability.

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

## Foundation

| Capability | Framework | Rust API | iOS native | In an app | iPhone | Documented | Limits |
|---|---|---|---|---|---|---|---|
| App-owned Rust crate with UniFFI async exports | Yes | Yes | Yes | Yes: starter, Pocket Library | No: built with xtool, install not recorded | Yes: app-crate.md | Chat still on the legacy shared FFI |
| Native SwiftUI structure (navigation, lists, sheets, search) | Yes: starter | n/a | Yes | Yes: starter, Pocket Library | No | Yes: ios-ui.md | Older examples use `ObservableObject` |
| Lifecycle phases reported to Rust | Yes: forge-runtime | Yes | Yes: `.forgeLifecycle` | Yes: starter | No | Yes | |
| Background work and cancellation in Rust | Yes: forge-runtime | Yes | n/a | Yes | No | Yes | Stops when iOS suspends the app |
| `forge.toml` app config (Info.plist, entitlements, xtool.yml) | Yes | n/a | n/a | Yes | Partial: builds only | Yes: app-config.md | No asset catalogs on Linux |

## Data and files (W1, W2)

| Capability | Framework | Rust API | iOS native | In an app | iPhone | Documented | Limits |
|---|---|---|---|---|---|---|---|
| JSON snapshot storage with migration hook | Yes: forge-storage | Yes | n/a | Yes: Chat, starter | Yes: Chat, 2026-11-05 | Yes | Rewrites the whole file on every change |
| Relational store with queries (SQLite) | No | No | n/a | No | No | No | S1: `rusqlite` bundled cross-compile with xtool unproven |
| Full-text search | No | No | n/a | No | No | No | Depends on SQLite FTS5 |
| Backup and restore archive | No | No | No | No | No | No | |
| App directories | Yes: forge-services | Yes | Yes | Partial: services check app only | No | Yes | Store relative paths only |
| Document picker import (copy into app) | Yes | Partial: path handed to Rust | Yes: `forgeFileImporter` | Partial: services check app only | No | Yes | |
| Share sheet and Save to Files | Yes | Yes: `ShareItem` | Yes: `ForgeShareLink` | Partial: services check app only | No | Yes | |
| Managed attachment store | No | No | n/a | No | No | No | |
| File-backed upload and download with progress | No | No | n/a | No | No | No | `forge-net` bodies are in memory |
| Background transfers | No | No | No | No | No | No | Needs native `URLSession` (S6) |
| CSV import and export | No | No | n/a | No | No | No | |
| Report export (zip, GPX, GeoJSON, PDF, PNG) | No | No | No | No | No | No | PDF and PNG drawn natively |

## Offline delivery (W11)

Chat's offline behaviour, tracked as its own capability because it tests the application architecture, not just networking.

| Capability | Framework | Rust API | iOS native | In an app | iPhone | Documented | Limits |
|---|---|---|---|---|---|---|---|
| Durable outgoing queue (sends, edits, deletes, reactions, uploads) | No | Partial: Chat's Sending and Failed states | n/a | Partial: Chat | No | Partial: chat-demo.md | Not yet stored before the network call, nor tested across a kill |
| Retry with exponential backoff and jitter | Partial: Chat stream reconnect | Partial | n/a | Partial: Chat streams; sends retry by hand | No | Partial | |
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
| Reachability | No | No | No | No | No | No | |
| Bonjour advertise and browse | No | No | No | No | No | No | Needs `NSBonjourServices` key |
| Peer-to-peer transfer with resume | No | No | No | No | No | No | |
| Device identity and encrypted channel | Partial: Keychain only | Partial | Partial | No | No | No | Use a reviewed library |
| Multicast discovery | No | No | No | No | No | No | Paid team plus Apple approval |

## Native services and permissions (W3, W4)

| Capability | Framework | Rust API | iOS native | In an app | iPhone | Documented | Limits |
|---|---|---|---|---|---|---|---|
| Keychain secrets | Yes | Yes | Yes | Partial: services check app only | No | Yes | Synchronous; call off the UI thread |
| Local notifications | Yes | Yes | Yes | Partial: services check app only | No | Yes | No push: APNs is a non-goal |
| Notification actions and grouping | No | No | No | No | No | No | |
| Deep links (custom URL schemes) | Yes | Yes | Yes | Partial: services check app only | No | Yes | Universal links need a paid team |
| Permission interface | Partial: notifications only | Partial | Partial | Partial: services check app only | No | Yes | `forge.toml` writes 20 permission strings the runtime cannot request yet |
| Microphone, camera, location, motion, local network, Bluetooth permissions | No | No | No | No | No | Partial: config only | |
| Photos picker | No | No | No | No | No | No | No permission needed |
| Camera capture | No | No | No | No | No | No | |
| Barcode and QR scanning | No | No | No | No | No | No | VisionKit scanner |
| Face ID app lock | No | No | No | No | No | Partial: config only | |
| Background refresh and processing tasks | Partial: config key | No | No | No | No | Partial | iOS decides when they run |
| Recurring schedules | No | No | n/a | No | No | No | |
| Haptics | n/a | n/a | Yes: SwiftUI | No | No | No | |

## Media, sensors and rendering (W5, W6, W8)

| Capability | Framework | Rust API | iOS native | In an app | iPhone | Documented | Limits |
|---|---|---|---|---|---|---|---|
| Audio session handling | No | No | No | No | No | No | |
| Record and play audio files | No | No | No | No | No | No | |
| Waveform summaries | No | No | n/a | No | No | No | |
| Real-time audio processing in Rust | No | No | No | No | No | No | UniFFI unusable on the render thread; S4 |
| Background audio | Partial: config | No | No | No | No | Partial | |
| Location fix | No | No | Partial: app code | Yes: Weather (one-shot) | Yes: Weather, 2026-11-06 | Partial: Weather README | Not a reusable service |
| Continuous location and tracks | No | No | No | No | No | No | |
| Background location | Partial: config | No | No | No | No | Partial | Needs "always" permission |
| Heading, altitude, motion | No | No | No | No | No | No | |
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
| Core ML | No | No | No | No | No | No | `.mlmodel` compiles only on a Mac |
| NFC | No | No | No | No | No | No | Paid team |

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
