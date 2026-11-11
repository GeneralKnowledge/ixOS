# Roadmap

Forge Mobile is an **integration reference**: a working Counter app on iOS (and on Android, which is work in progress), a CLI, templates, and the docs that explain each piece. It is actively maintained. This page says what comes next, in rough priority order, so you can tell what is planned and where help is welcome.

Status labels: **In progress** (someone is on it), **Planned** (agreed, not started), **Open** (direction agreed, contributions welcome), **Not planned** (out of scope for now, with the reason).

Last reviewed: 2026-11-09.

What each capability has actually proven, from "has an API" to "recorded working on an iPhone", is tracked in the [capability ledger](capability-ledger.md).

What the demos have built that should move into the framework, and in what order, is in the [framework extraction audit](framework-extraction-audit.md).

## Product direction: a native application framework

Decided 2026-11-09 (Reuben): ixOS is a native application framework with a Rust application core and a SwiftUI design system, not a Rust-to-iOS bridge with increasingly sophisticated tools. The test for every feature: does it make it easier to build a complete, attractive, reliable app? SwiftUI stays the UI framework (no new renderer, DSL, navigation or state framework); Forge adds a design system and application patterns on top of it; Rust owns domain rules, validation and persistence; the CLI builds and operates the app, and agents are optional.

| Priority | Work | Status |
|---|---|---|
| P0 | Existing runtime and native integration stability | Maintain |
| P1 | **ForgeUI 0.1**: tokens, themes, core components, a showcase app ([proposal](research/forgeui-proposal.md), [ADR 002](adr/002-forgeui-in-forgekit.md), [guide](forgeui.md)) | **0.1 done: run on an iPhone 2026-11-10** |
| P2 | **ForgeAppKit 0.1**: alerts and confirmations as model values, destinations, load and submission state, form errors ([guide](forgeappkit.md)) | **In progress: 0.1 built, tested on Linux, not yet on an iPhone** |
| P3 | Rust application foundations: saved documents and validation ([guide](app-foundations.md)); repositories and query later (modules in existing crates first) | **In progress: document and validation in `forge-app`** |
| P4 | A complete reference app (personal library) built only on the above | Required proof |
| P5 | Developer experience: `forge init` on ForgeUI, component docs, feature templates | After the foundation |
| P6 | Autonomous-agent tooling expansion (M6, general device control) | **Deferred.** Device, build, diagnostics and MCP work stays as supporting infrastructure |

No Android work and no cross-platform components until the iOS experience is right.

Native capabilities (web view, documents, media, background tasks, identity, system integration such as universal links, share extensions and App Intents, offline data, maps, charts) are planned in [native-capability-roadmap.md](native-capability-roadmap.md): ixOS wraps Apple's frameworks rather than replacing them, proves each wrapper in the services check app and on an iPhone, and judges usability in real apps (Pocket Library, Chat, and a feed reader benchmarked against NetNewsWire as the reader's second consumer). Its first tier is `forge-services` work and runs alongside P1 and P2.

## Platform focus

### Android — Work in progress

Decided 2026-11-07: ixOS focuses on **iOS**. Android is paused as work in progress:

- The Android code stays: `examples/counter-android`, `examples/chat-android`, `templates/android-app`, `forge init --platform android`, `forge build android` and the `ForgeAndroid` HTTPS init in `forge-runtime`.
- It keeps building. The Android CI job runs when Android or shared Rust code changes, so shared changes that break it are still caught.
- It gets no new features. New capabilities (native services, Chat milestones, the demo sequence) are built and proven on iOS only, and the [capability ledger](capability-ledger.md) tracks iOS only.
- It has not been validated on a physical Android phone, and none of the `forge-services` interfaces has an Android implementation.

Rust interfaces stay platform-neutral, so Android implementations can be added when the work resumes. Contributions that keep Android building are welcome; larger Android work should wait until this section changes.

### Tooling freeze and reuse rules

Decided 2026-11-11 (Reuben), from the [reinvention audit](research/reinvention-audit.md). The framework mostly builds on existing tools; the custom code is in the tooling, so:

1. **Agent and device tooling is frozen** until P1 to P4 are done: `forge ui explore`, `forge ui focus`, run records (`forge verify --run`), `forge mcp` and the live dev connection (`forge device pair|connect|cmd`). Bug fixes only, no new commands or features. They keep working and keep their tests.
2. **iOS networking moves to URLSession** ([native capability roadmap](native-capability-roadmap.md)). `forge-net`'s portable types stay; ureq stays for Linux tools, tests and Android.
3. **A new native service gets a Rust interface only when the Rust core reads its value.** Otherwise the Swift app calls Apple's framework directly ([architecture.md](architecture.md#framework-principles)).
4. **Evidence bundles also write JUnit XML** (`junit.xml`, [evidence.md](evidence.md#the-evidence-bundle)), so any CI system or test viewer can read the results. No new report features.

The freeze ends when all four of these exit criteria are met, each checked on a physical iPhone and reported by a person:

| | Done when | Status (2026-11-11) |
|---|---|---|
| P1 ForgeUI 0.1 | The showcase's settings, list and create/edit form screens use no custom styling and run on an iPhone in light, dark and the largest text size | **Met** 2026-11-10 |
| P2 ForgeAppKit 0.1 | Pocket Library uses its alerts, destinations, load and submission state and form errors, and its UI tests that cover them pass on an iPhone | Not met: built and tested on Linux only |
| P3 Rust foundations | Persistence guarantees ([#159](https://github.com/GeneralKnowledge/ixOS/pull/159)) are merged, Pocket Library runs on the `forge-app` `Runtime`, and its saved-version fixtures open in CI | Not met: #159 open, Pocket Library uses only validation |
| P4 Reference app | Every requirement in Pocket Library's `requirements.toml` passes on an iPhone in one `forge ui run` sitting, with no app-local copies of framework code | Not met |

Whatever happens, the freeze is reviewed on 2026-12-15, so it can't drift into an indefinite one. At the review, it either ends, or gets a new date and the reason it still holds.

## Near term

### Autonomous dev loop (Forge M6) — Frozen

Frozen on 2026-11-11 ([above](#tooling-freeze-and-reuse-rules)): bug fixes only until P1 to P4 are done.


An agent takes a real task on an existing app, changes it, checks it on the iPhone, finds and fixes a real problem, and leaves one record of what ran and what didn't. Most of the pieces exist; M6 wires them together, aims exploration at what changed, correlates failures with logs and crashes, and demonstrates the loop on Good Dog. Plan: [forge-m6.md](forge-m6.md).

### iPhone acceptance of the integrated framework — In progress

The native starter UI (#32), developer experience (#35), lifecycle (#36), native services (#34) and the Pocket Library acceptance app (#33) are integrated and green in CI together ([integration report](integration/pr-integration-report.md)). What is left is the physical-device run: [integration/iphone-acceptance.md](integration/iphone-acceptance.md) is the single runbook, from a fresh `forge init` app to a Rust change reaching the phone. So far the starter, the services check and Pocket Library have been built with a patched xtool on Linux (iOS SDK 26.0 recorded); none has been recorded running on an iPhone yet.

### Native iOS UI — Done, device check open

Done: the `forge init` starter is a multi-screen SwiftUI app (list with Rust search, detail with edit in place, compose sheet, settings) on an `@Observable` model, and [ios-ui.md](ios-ui.md) is the standard for building on it. It compiles with xtool on Linux. Open: the device checks in the acceptance runbook (section B).

### Developer CLI polish — Done, follow-ups open

Done: `forge init --path`, with each app owning its Rust crate ([app-crate.md](app-crate.md)); `forge check` (the local CI matrix); `forge doctor`, which reports what each workflow needs with a fix for each missing piece, and `--json` for CI; `forge bindings generate --project`; and `--project` (for build, run and bindings) defaulting to the current app directory.

Done (M5 developer experience): `forge init MyApp` creates `./MyApp` outside the checkout and prints the next steps, and every generated app has a README; `forge doctor` checks the Apple sign-in, connected iPhones and the iOS SDK version; `forge build ios` / `forge run ios` check the toolchain, sign-in and iPhone before building and explain failed Rust, Swift, signing and install steps; `forge test` runs the app's own tests from its directory and skips XCTest on Linux. See [getting-started.md](getting-started.md).

Open: provisioning profiles, Developer Mode and SDK licences are diagnosed from the failure, not checked beforehand; Android installs (`adb`) get no failure diagnosis yet.

### iOS app configuration — Done, follow-ups open

Done: `forge.toml` is the one place for an iOS app's display name, version and build number, deployment target, icon, orientations, permission prompts, URL schemes, background modes, entitlements and signing team type. Forge generates Info.plist, App.entitlements and xtool.yml from it (and keeps Package.swift's `.iOS(...)` in step) on every build, `forge config check` guards it in CI, and `forge build/run ios --release` builds the release configuration ([app-config.md](app-config.md)).

Open: asset-catalog icons and launch screens (xtool cannot compile asset catalogs on Linux), per-configuration overrides (for example a separate debug bundle ID), app extensions (xtool supports them; Forge has no template), and applying `team_id` / `minimum_ios` to the Xcode project instead of reporting the difference.

### AGP / Gradle stabilization — Paused

Android builds are pinned to **AGP 8.7.3 + Gradle 8.10.2** (see [compatibility.md](compatibility.md)). Paused while Android is work in progress. A Dependabot bump to AGP 9 / Gradle 9 broke the Android CI job and was rolled back, in the example and the template. The plan:

1. Migrate `examples/counter-android` and `templates/android-app` together, in one PR, with the Android CI job green.
2. Keep the example and template plugin versions identical (they are the same contract).
3. Update the compatibility matrix in the same PR.

Until then, Dependabot Gradle PRs that cross a major AGP or Gradle version should be closed, not merged.

### xtool / Xcode compatibility plan — Planned

Linux iOS builds depend on [xtool](https://github.com/xtool-org/xtool) and the Darwin SDK it extracts from an `Xcode.xip`. Both move independently of this repo, so:

- **Pin what is tested.** [compatibility.md](compatibility.md) lists the xtool, Swift and Xcode versions that have been exercised, and which ones have only been installed but not run end to end.
- **Record real runs.** A version is promoted to "verified" only after the manual device checklist in [toolchain-validation.md](toolchain-validation.md) passes with it.
- **Isolate xtool.** The iOS build steps sit behind an `IosBuilder` trait in `forge-cli` (done), so an xtool break, or a second backend (plain `xcodebuild` on macOS), touches one module instead of the whole CLI.
- **macOS stays a supported path.** CI already builds the Xcode projects on `macos-15`, so a Mac with Xcode is the fallback when an xtool release regresses.

## Medium term

### Networking — In progress

`forge-net` has a transport trait, test doubles, and a real blocking HTTPS client (`UreqClient`: ureq + rustls with the platform certificate verifier) behind its `ureq` feature. The iOS Weather example ([examples/weather](../examples/weather/README.md)) uses it end to end: Open-Meteo requests from Rust, typed errors, cancellation, and an offline cache. The Chat demo ([docs/chat-demo.md](chat-demo.md)) uses it on iOS (and on its frozen Android twin): a long-lived Server-Sent Events stream per room, reconnects with catch-up, and idempotent retries, against a local mock server.

HTTPS works on Android too (work in progress): hosts call `ForgeAndroid.init(applicationContext)` once, and certificates are checked against Android's trust store, with typed `Certificate` errors ([android-setup.md](android-setup.md#https-certificate-verification)). It is covered by Rust tests against local TLS servers and the JVM test of the JNI entry point; a run on a physical Android phone is still to be recorded.

Ordinary requests can be interrupted in flight: `HttpClient::send_cancellable` closes the connection, and the starter app crate wires it to Swift task cancellation and Kotlin coroutine cancellation ([app-crate.md](app-crate.md#lifecycle)). Apps also report the OS lifecycle to Rust (`set_phase`), which stops on-screen requests in the background and announces resume; device runs of it are not yet recorded. The Weather example still discards a late response instead of interrupting it. Still open: platform reachability.

**Moving iOS to URLSession** ([native-capability-roadmap.md](native-capability-roadmap.md#where-current-code-departs-from-native-first), decided 2026-11-10). Background downloads come first and are built: the `transfers` service runs them in a background `URLSession` while Rust keeps the ledger, checks each file and recovers after a relaunch ([native-services.md](native-services.md#background-transfers-transfers)); their phone checks are owed. Ordinary requests are next and built as the `http` service: a `URLSession` client behind `HttpClient`, opt-in, with Commonplace's feed fetches as its first user. Then streaming, then `URLSession` by default on iOS. ureq stays for Linux tests and Android.

### iOS Simulator support — Open

Builds are **device-only** today. The Rust library ships as a single `aarch64-apple-ios` static library ([ADR 001](adr/001-ios-static-linking.md)), and the iOS Simulator only runs on macOS, so in the Linux-first workflow a physical device is the only way to run the app anyway. Simulator support mainly helps Mac users and macOS CI. It needs:

1. An `aarch64-apple-ios-sim` (and optionally `x86_64-apple-ios`) slice.
2. Packaging that can hold more than one slice: an XCFramework or a SwiftPM artifact bundle (ADR 001's fallback).
3. `ios_simulator = true` in `forge.toml` wired through the CLI (config parsing exists; the build path does not).
4. A macOS CI job that runs the XCTest suite on a simulator.

Good first project for someone with a Mac.

### Native iOS services (M3) — In progress

`crates/forge-services` gives Rust apps Keychain secrets, app directories and file import, local notifications, a single permission interface, the share sheet, and deep links. Each is a small Rust interface with a Swift implementation that Forge ships, and every `forge init` app has them installed ([native-services.md](native-services.md)). They are covered by Rust tests, macOS XCTest and an unsigned iOS build in CI. An iPhone run of [the device check](native-services-device-check.md) is still to be recorded. Next: the tiers in [native-capability-roadmap.md](native-capability-roadmap.md), starting with the web view, documents, the native reader, photos and camera, and background tasks. Android implementations of the same interfaces wait until Android work resumes ([above](#android--work-in-progress)).

### Store distribution (App Store, Google Play) — Open

Not in this release. Today the CLI produces development builds (an xtool debug build signed for your own device on iOS, a debug APK for Android; see [builds.md](builds.md)). Release distribution needs:

- **Android:** release build type, signing config read from the environment (never committed), an `.aab` bundle, and R8/minification rules that keep JNA and the generated bindings.
- **iOS:** an archive and export path. App Store submission needs a distribution certificate and provisioning profile; whether xtool can produce a submission-ready build on Linux has not been verified, so the first deliverable is a documented macOS `xcodebuild archive` path.
- A `docs/distribution.md` covering both, plus release notes in [release-process.md](release-process.md).

The staged plan for the first real iOS release (Pocket Library; what Linux can do and what needs a Mac; `forge release check`) is [first-app-store-release.md](first-app-store-release.md).

### Any app on the iPhone (growing UI testing) — Frozen

Frozen on 2026-11-11 ([above](#tooling-freeze-and-reuse-rules)); the spikes below wait.


Forge UI Testing's routes (WebDriverAgent, Apple's accessibility service, `idevice` device services) used on apps Forge didn't build: observe, operate, and, the bigger goal, explore and verify with evidence. Not a separate stack: the same `Driver`, `UiNode` and `Locator`, with a person-written per-app policy for anything outside the project. Plan, safety model and an evaluation of iCTRL: [research/general-device-control.md](research/general-device-control.md). After the current milestone; four no-install spikes go first.

## Future

### Split Forge into focused projects — Future

ixOS carries several products in one repository. The idea is to separate them by purpose, each with one user and one measure of success: **Forge** (the framework: Rust core, ForgeKit, ForgeUI, ForgeAppKit, releases), **Forge CLI** (the development workflow), **Forge Device** (device access, UI testing, exploration), **Forge Agents** (MCP server and agent setup, optional) and **Forge Examples** (demo apps and benchmarks). The framework would depend on none of the others, and apps would use Forge without cloning this repository. Boundaries would come first (a scope page per project), and code would move only where a release cycle needs it. One question comes before any restructuring: is Forge Device a general testing product or a Forge subsystem? The proposal, with how today's code maps onto it, is in the [Forge product boundaries](https://claude.ai/artifact/YBwLCg9UewZtk5cfkYwNNY) document. Nothing is scheduled.

### Future ideas with no AI needed — Future (P5)

From the [app-generation ideas assessment](https://claude.ai/artifact/TahwUyxU2CLEGH5yakJfMc) (2026-11-11). Each is a deterministic `forge` command or template, works on Linux, needs no AI agent, and waits until P2 to P4 are met so it copies settled patterns. None touches the frozen tooling.

1. **Feature scaffolding** (`forge add feature <Name> --field title:text ...`): writes a whole feature for a record with simple fields: Rust record, validation, persistence and commands, Rust tests, a view model, ForgeUI list, detail and create/edit screens, model tests for `forge check swift`, a `uitests/*.toml` scenario and requirement lines, then regenerates bindings. Its specification is [feature-recipe.md](feature-recipe.md), taken from Pocket Library and checked against a second app first. Rough effort 2 to 3 weeks.
2. **Adding a capability** (`forge add service <name>`): one command that adds the `native_services!` entry (only when the Rust core reads the value), the `forge.toml` permission or entitlement, the Swift adapter wiring and a test stub, and reminds you to update the ledger. Bindings and Info.plist are already regenerated on every build, and `forge config check` already prints the exact `forge.toml` line for a missing permission. Rough effort 1 week.
3. **Faster visual iteration on the iPhone**: measure how long a one-line SwiftUI change takes to reach the screen; move Pocket Library's test-data mode into the starter; a launch option that opens the app on a named screen with fixture data; a debug-build gallery of the app's own screens in light, dark and the largest text size; and an incremental build that skips the Rust step when only Swift changed. The iOS Simulator and hot reload need a Mac and are not part of this. Rough effort 1 week.

The AI-driven ideas from the same assessment (screen generation from a description or image, app generation from a specification) are agent work on top of these, and wait for the freeze review.

## Not planned (for now)

- **Windows development hosts.** Tooling targets Linux and macOS.
- **A generic event bus or callback interfaces across the FFI boundary.** Events are pulled through bounded subscriptions; see [ffi-policy.md](ffi-policy.md#concurrency-contract) for why.
- **Publishing crates to crates.io.** The workspace is source-only until the APIs settle.

## How to pick something up

Open an issue saying which item you want to work on and how you plan to approach it, then follow [CONTRIBUTING.md](../CONTRIBUTING.md). For anything touching the FFI boundary, read [ffi-policy.md](ffi-policy.md) first; for packaging or architecture decisions, add an ADR under [adr/](adr/).
