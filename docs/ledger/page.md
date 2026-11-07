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

<!-- ledger: module modules -->

## Foundation

<!-- ledger: capability foundation -->

## Data and files (W1, W2)

<!-- ledger: capability data-and-files -->

## Offline delivery (W11)

Chat's offline behaviour, tracked as its own capability because it tests the application architecture, not just networking.

<!-- ledger: capability offline-delivery -->

## Networking (W7)

<!-- ledger: capability networking -->

## Native services and permissions (W3, W4)

<!-- ledger: capability native-services -->

## Media, sensors and rendering (W5, W6, W8)

<!-- ledger: capability media-sensors-rendering -->

## Extensions and restricted frameworks (W9, W10)

<!-- ledger: capability extensions -->

## Forge Device (W12)

Developer tooling on the Linux computer, not app capabilities, so "Rust API" and "iOS native" are n/a. "In an app" means used against an app on the phone.

<!-- ledger: capability forge-device -->

## Performance

A capability can work and still give a poor app. Each metric has a proposed target, a way to measure it from Linux with xtool (timestamps the app logs, read from the device log, or a diagnostics screen in the app), and the last recorded figure. Targets are proposals until the first measurement on the test iPhone, then they are revised once and kept.

<!-- ledger: metric performance -->

## Developer experience: build without the maintainer

The day-to-day loop (create, change, build, launch, inspect, diagnose) is assessed in [dev-loop.md](dev-loop.md), with measured times and the gaps that block it.

The demos prove that we can build apps with ixOS, not that someone else can. In this exercise a developer who has not worked on ixOS gets a clean Ubuntu machine, the docs and an iPhone, and nothing else. It runs first after Chat M6 and again after the sixth demo.

<!-- ledger: dx_step developer-experience -->

Also recorded for each run: time to the first successful iPhone build, every time they had to read framework source, missing or wrong documentation, confusing APIs, and any platform workaround they needed.
