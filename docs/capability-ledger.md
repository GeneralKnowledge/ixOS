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
| Documented | A page in `docs/` explains it for third-party developers |
| Limits | What does not work, or needs a paid team, a Mac, or a spike |

"Services check app" is the throwaway app `scripts/make-services-check-app.sh` builds for [native-services-device-check.md](native-services-device-check.md).

Values: **Yes**, **Partial** (with a note), **No**, **n/a**. Workstream ids (W1 to W10) and spike ids (S1 to S6) refer to the plan.

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
