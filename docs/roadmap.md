# Roadmap

Forge Mobile is a **native app framework** with working example apps on iOS (and Pocket Library on Android, which is work in progress), a CLI, templates, and the docs that explain each piece. It is actively maintained. This page says what comes next, in rough priority order, so you can tell what is planned and where help is welcome.

Status labels: **In progress** (someone is on it), **Planned** (agreed, not started), **Open** (direction agreed, contributions welcome), **Not planned** (out of scope for now, with the reason).

Last reviewed: 2026-11-15. Which apps still lag behind the framework, and which wave catches them up, is in [app-parity.md](app-parity.md).

What each capability has actually proven, from "has an API" to "recorded working on an iPhone", is tracked in the [capability ledger](capability-ledger.md).

What the demos have built that should move into the framework, and in what order, is in the [framework extraction audit](https://github.com/GeneralKnowledge/ixOS/blob/8fdd497cc51dfd0a8220f7dc013c3a25515f6c51/docs/framework-extraction-audit.md).

## Apps first: prove, complete, then extract

Decided 2026-11-15 (Reuben): **prove the apps before expanding them. Complete the apps before expanding the framework. Extract only what real applications have independently demonstrated they need.** No new example apps, no app catalogue and no new modules for now; the apps in `examples/` are the work. Plan: [Apps first: catch-up and completeness plan](https://claude.ai/artifact/LPStN1ZT8Uk63b4Jdea7KH).

**The order.** Each step is a gate: the next starts when the one before meets its criteria.

| Step | Work | Done when |
|---|---|---|
| 1. Prove what is merged | One combined Device Lab build from main and one phone sitting | Every app in the baseline of [device-validation.md](device-validation.md) has Compiles, Installs, Launches and Main interactions recorded by a person, with its problems listed. Sketchbook, Soundscape and Relay R2 matter most: they have never run on an iPhone |
| 2. Catch up (wave 3) | Weather, then Good Dog, then Chat onto ForgeHost (Good Dog and Chat also onto `forgeAlert`); requirements for Sketchbook and Soundscape and the missing device scenarios | [app-parity.md](app-parity.md) lists no wave 3 lag, and each moved app has its own row in device-validation.md |
| 3. Complete the apps (wave 4 on) | One user-facing feature at a time, per app, with the loop below | Per feature, the completeness criteria below |

Device Lab's own state model is **not applicable**, not lag: its state is lab services, not an app core. Relay's error conversions stay deferred until its iroh rework. Sketchbook gets no new features until its first device validation.

**Acceptance criteria for each wave 3 PR.** Each app moves in its own PR, and the next app starts only when the one before is proven:

1. One app per PR. A mechanical cleanup shared by several apps is its own PR, after the moves it depends on.
2. The app's own lifecycle and alert copies are deleted, not kept beside the framework ones, and `scripts/check-app-parity.sh` loses that app's wave 3 lines.
3. Behaviour is unchanged unless the PR says what changed and why. Its requirements and UI scenarios still pass (`cargo test`, the headless twin), and new ones cover what the move touched (load failure and Try Again, at least).
4. It compiles for an iPhone (`scripts/check-ios-xtool.sh affected`), recorded in the PR.
5. A person runs it from Device Lab on the iPhone, and the result is a row in [device-validation.md](device-validation.md): launches, main interactions, relaunch after backgrounding, and the failure screen if it can be forced.
6. Friction met during the move goes into the [app friction log](research/app-friction-log.md).

**The completeness loop (wave 4 on).** No fixed feature list. The current list of next features per app, and the framework each needs, is in [app-plan.md](app-plan.md) (2026-11-21). For each app, repeatedly:

1. Pick the most noticeable thing a user of the app would miss.
2. Build it in the app.
3. Compile it for an iPhone.
4. Test it on the iPhone and record it in device-validation.md.
5. Record friction and framework opportunities in the friction log.
6. Only then choose the app's next feature.

The current first picks: Good Dog reminders and progress over time (training history, sessions done, progress per exercise; streaks optional and never the main measure); FieldKit elevation and pace charts, then background recording, then photo and voice notes; Pocket Library full-text search inside books and notes (matching passages, jumping to the place in the book, filters for books and notes); Commonplace folders and OPML (the start of its feed reader, below); Relay Send with a Code; Soundscape mixdown sharing after its first device run; Weather an hourly chart after its move. Chat and Sketchbook get no new features until their next device validation.

**A feature is complete** when it works for real data and its unhappy paths, not when it first renders:

- Empty, loading and error states, with a way to recover from the error.
- Real-world data sizes (a long book, a long track, months of history), not just fixtures.
- The permissions it needs: asked at the moment of use, and a useful screen when they are refused.
- Changes and cancellation: what happens when the user edits, undoes or turns it off (a changed reminder schedule replaces the old one; a cancelled search stops).
- Requirements in `requirements.toml` with tests, a UI scenario, accessibility identifiers and labels, and a device-validation row.

**Extraction gates.** The framework earns each abstraction through use:

1. **The first implementation stays in the app,** in the simplest maintainable form.
2. **A second app establishes the need.** When a second app needs the same capability, compare both implementations and name what is truly common.
3. **Extract only the common part,** after the second app's feature has merged and been validated on an iPhone. Don't force the apps into the same product behaviour because they share plumbing: two apps can share chart styling without sharing chart data or presentation.

Expected from the first picks: full-text search (Commonplace, then Pocket Library) into forge-storage; a chart style (Good Dog and FieldKit, then Weather) into ForgeUI; the photo picker (Chat, then FieldKit) into ForgeNative.

**Waves 4 and 5** (decided 2026-11-16, Reuben). They start after the wave 3 moves have merged.

- **Wave 4: extractions whose second use already exists.**
  - The read connection that skips the revision, used by Commonplace and FieldKit ([friction log](research/app-friction-log.md) A5).
  - Small leftovers: init naming, template leftovers (A21), id types (A3) and distance maths in the services crate.
  - The phase hook for apps (A31) waits for Relay to need it too.
  - Not Relay's encrypted channel (A16): Relay's approved plan replaces that wire with iroh ([relay-transfer-plan.md](../examples/relay/docs/relay-transfer-plan.md)), which leaves the channel with one user, the device tools.
- **Wave 5: give each one-use piece its second app,** as a real feature with its own phone row, one app per PR, in this order:
  1. Full-text search, in three steps:
     1. Commonplace's search across collected articles (it already has an FTS5 search) is validated on an iPhone.
     2. Pocket Library builds and validates its own search across books and notes.
     3. The two are compared, and only what they genuinely share moves into forge-storage.
  2. FieldKit's elevation and pace charts and Good Dog's progress over time, then Weather's hourly chart.
  3. FieldKit's photo notes, after Chat's photo picker. **The photo picker is the next framework extraction in the queue** (noted 2026-11-16): once FieldKit's picker passes on the iPhone, the part it shares with Chat's moves into the framework, and Soundscape's song artwork is the first feature built on the extracted picker rather than its own copy.

  The shared part moves into the framework after both features have been validated on an iPhone.

**Everyday capabilities** (agreed 2026-11-16, Reuben; [audit and plan](research/everyday-capabilities.md)). Five small completions of what apps repeat, each proven by an app feature, after this docs pass (phase 0) and one at a time. None starts as a framework project:

| Phase | Work | Proven by | Starts with |
|---|---|---|---|
| 1 | Unreadable saved data has a way forward: Start Fresh for corrupt data, never for newer data | Sketchbook, then Pocket Library | A design note: the typed open failure reaching `ForgeRoot` |
| 2 | A refused permission never leaves a dead screen | FieldKit, Sketchbook, Relay | The SwiftUI denied state |
| 3 | Offline files kept but not backed up; background transfers in a real feature | Soundscape offline downloads | Where the songs live |
| 4 | One way to add a native service, usable outside ixOS | Soundscape and Weather first | An investigation, then a yes or no |
| 5 | A shared Now Playing helper | Soundscape, Pocket Library | Comparing the two playback interfaces |

**Expand an existing app before adding a new one** (decided 2026-11-16, Reuben). A missing application category does not by itself justify a new app. First ask whether an existing app has a real reason to grow into it; if one does, that growth proves the capability.

- **Feed reading belongs to Commonplace.** There is no separate feed reader app. Commonplace grows into a feed reader and personal reading system:
  - subscriptions and a unified chronological feed;
  - full article extraction and background refresh;
  - OPML import and export, folders, and read, unread, favourites and history;
  - search across what it has collected.
- **Planning and time are spread across the apps, each in its own way.** There is no generic shared calendar:
  - Good Dog plans activities: training schedules, recurring sessions, history, and appointments in the calendar.
  - Checklist schedules obligations: due dates, recurring tasks, reminders and an agenda.
  - Commonplace organises information over time: daily notes, reading queues, follow-up dates and resurfacing.
  - FieldKit plans and records fieldwork: planned trips, scheduled sessions and a time-stamped history.
  - Pocket Library tracks reading intentions: goals, schedules, reminders to resume and statistics.

  These are candidate first and second users of each capability, not an order to extract:

| Capability | First user | Possible second user |
|---|---|---|
| Local notifications | Good Dog | Checklist |
| Recurring schedules | Checklist | Good Dog |
| Calendar integration (Apple's Calendar through EventKit) | Good Dog | FieldKit |
| Widgets | Good Dog | Checklist |
| App Intents and Shortcuts | Checklist | Pocket Library |
| Timeline presentation | Commonplace | FieldKit |
| Background scheduled work | Commonplace | Good Dog |

"Calendar" here means only integration with Apple's Calendar (EventKit): adding an appointment or a planned trip to the person's calendar. It is not an app's own agenda or calendar screen, which stays each app's own design, and not notifications or recurring schedules, which are their own rows. Two apps showing dates is no reason for a shared calendar abstraction.

## Product direction: a native application framework

Decided 2026-11-09 (Reuben): ixOS is a native application framework with a Rust application core and a SwiftUI design system, not a Rust-to-iOS bridge with increasingly sophisticated tools. The test for every feature: does it make it easier to build a complete, attractive, reliable app? SwiftUI stays the UI framework (no new renderer, DSL, navigation or state framework); Forge adds a design system and application patterns on top of it; Rust owns domain rules, validation and persistence; the CLI builds and operates the app, and agents are optional.

| Priority | Work | Status |
|---|---|---|
| P0 | Existing runtime and native integration stability | Maintain |
| P1 | **ForgeUI 0.1**: tokens, themes, core components, a showcase app ([proposal](https://github.com/GeneralKnowledge/ixOS/blob/8fdd497cc51dfd0a8220f7dc013c3a25515f6c51/docs/research/forgeui-proposal.md), [ADR 002](adr/002-forgeui-in-forgekit.md), [guide](forgeui.md)) | **0.1 done: run on an iPhone 2026-11-10** |
| P2 | **ForgeAppKit 0.1**: alerts and confirmations as model values, destinations, load and submission state, form errors ([guide](forgeappkit.md)) | **In progress: 0.1 built and used by Pocket Library, Commonplace, FieldKit, Relay, Sketchbook and Soundscape; not yet recorded on an iPhone** |
| P3 | Rust application foundations: saved documents and validation ([guide](app-crate.md#saved-documents-and-validation)); repositories and query later (modules in existing crates first) | **Built: persistence guarantees (#159) and Pocket Library on the runtime (#176); iPhone check open** |
| P4 | A complete reference app (personal library) built only on the above | Required proof |
| P5 | Developer experience: `forge init` on ForgeUI, component docs, feature templates | After the foundation |
| P6 | Autonomous-agent tooling expansion (M6, general device control) | **Moved out** to the [forge-lab](https://github.com/GeneralKnowledge/forge-lab) side project (2026-11-15). Device, build and diagnostics stay here as supporting infrastructure |

No cross-platform components until the iOS experience is right. Android runs as a capped side track (below): one milestone at a time, never ahead of the iOS gates.

Native capabilities (web view, documents, media, background tasks, identity, system integration such as universal links, share extensions and App Intents, offline data, maps, charts) are planned in [native-capability-roadmap.md](native-capability-roadmap.md): ixOS wraps Apple's frameworks rather than replacing them, proves each wrapper in the services check app and on an iPhone, and judges usability in real apps (Pocket Library, Chat, and a feed reader benchmarked against NetNewsWire as the reader's second consumer). Its first tier is `forge-services` work and runs alongside P1 and P2.

## Platform focus

### Android — Work in progress

Decided 2026-11-07: ixOS focuses on **iOS**. Decided 2026-11-20 (Reuben): Android moves forward as a **capped side track**, following Pocket Library only.

- iOS comes first. One Android milestone is in progress at a time, and iPhone sittings and the [Apps first](#apps-first-prove-complete-then-extract) gates always go before it.
- Only Pocket Library gets an Android version (`examples/pocket-library-android`). No other app is ported, and new capabilities are still designed and proven on iOS first.
- Apps first holds on Android too: the Kotlin host, the ForgeAppKit counterparts, the theme and the service adapters live inside the Android app until a second Android app needs them.
- Results come from an emulator in CI. Nothing has run on a physical Android phone, and the [capability ledger](capability-ledger.md) tracks iOS only.
- `templates/android-app`, `forge init --platform android`, `forge build android` and the `ForgeAndroid` HTTPS init stay and keep building.

**Android parity plan** (detailed plan agreed 2026-11-20, [doc](https://claude.ai/artifact/X6StvqebB5T4Fa34bDPohC)). Parity means Pocket Library on Android meets the same requirements (PL-1 to PL-10) as on iOS, as Compose instrumented tests on the CI emulator. Rust stays the one shared core; Android's interface is Jetpack Compose with Material 3, with no shared UI code. One PR per milestone; each starts when the one before has merged with its requirements green.

| Milestone | Work | Requirements | Size | Status |
|---|---|---|---|---|
| Stage 0 | Pocket Library's own Rust core under a minimal Compose app (add, open, delete), a Kotlin `ForgeHost`, a Compose UI test on a CI emulator | | 3 to 5 days | **Done** (#183) |
| A1 | App structure (typed navigation, process lifecycle), Kotlin counterparts of ForgeAppKit's failure, alert, load, submission and field errors, error text for every error | PL-4, PL-10 | 1 week | **Done** (#349) |
| A2 | ForgeUI's Swift tokens generate the Android theme's tokens (`scripts/android-tokens.py`, checked in CI); Material 3 colours and type from them; contrast tests | | 1 to 1.5 weeks | **Done** (#350) |
| A3 | The complete library: full book form, edit, progress, search, filter and sort, start fresh | PL-1, PL-2, PL-3, PL-5, PL-6 | 1.5 weeks | **Done** (#355) |
| A4 | Android `http` service adapter and the catalogue | PL-7, PL-8 | 1.5 weeks | **Done** (#359) |
| A5 | EPUB reader and read aloud (`TextToSpeech`) | PL-9 | 2 weeks | **Done** (#368) |
| A6 | Background work (WorkManager), settings, backups | PL-1 after process death | 1 week | In progress (#371) |
| A7 | AI summaries shown as unavailable (no on-device model on the emulator) | | 2 to 3 days | Planned |
| A8 | `forge run android --emulator` and logcat | | 2 to 3 days | Planned |

After A8 (decided 2026-11-21, Reuben): **on-device AI for Android** as its own stage. R0 checks the candidate projects and engines, R1 compares two engines in a check app, and R2 adds the chosen one as a Kotlin `AiModel` (Forge AI's foreign trait) for Pocket Library's summaries. Speed results count only from a physical phone. Plan and findings: [research/android-local-ai.md](research/android-local-ai.md).

### Tooling freeze and reuse rules

Decided 2026-11-11 (Reuben), from the [reinvention audit](research/reinvention-audit.md). The framework mostly builds on existing tools; the custom code is in the tooling, so:

1. **Agent tooling moved out; device tooling is frozen.** On 2026-11-15 (Reuben, from the complexity audit) `forge ui explore`, `forge ui focus`, run records (`forge verify --run`), `forge mcp` and the live dev connection (`forge device pair|connect|cmd`) moved to the [forge-lab](https://github.com/GeneralKnowledge/forge-lab) side project, which depends on ixOS and can grow on its own. They don't come back into ixOS. What stays (`forge ui`, `forge device`, `forge crash`) gets bug fixes only, no new commands, until P1 to P4 are done. One narrow exception, agreed 2026-11-11 (Reuben): **interaction completeness**. The basic actions a developer expects when driving an iPhone (swipe, long press, double tap, drag, keys, scrolling a lazy list, then Home, buttons, open URL and system alerts) are added to `forge-uitest`, test files and one-step `forge ui` commands, and forge-lab's `forge_ui_act` can use the same actions. Nothing from the autonomous-agent work. Plan: [Forge Device interaction completeness](https://claude.ai/artifact/9VJxnBUn6aoSMzmsTkbnVo); I1 (basic interaction), I2 (Home, buttons, open URL, alerts, back, app state, permissions), I3 (tree filters, `expect` visible, hittable, selected and count, settle, pickers) and I4 (pinch, rotate, two-finger tap, orientation, lock) are built. Coordinate taps and swipes are terminal-only for a person (`--at`, `--from`/`--to`); test files and agents refuse them.
2. **iOS networking moves to URLSession** ([native capability roadmap](native-capability-roadmap.md)). `forge-net`'s portable types stay; ureq stays for Linux tools, tests and Android.
3. **A new native service gets a Rust interface only when the Rust core reads its value.** Otherwise the Swift app calls Apple's framework directly ([architecture.md](architecture.md#framework-principles)).
4. **Evidence bundles also write JUnit XML** (`junit.xml`, [evidence.md](evidence.md#the-evidence-bundle)), so any CI system or test viewer can read the results. No new report features.
5. **The build queue is finished** (decided 2026-11-15, from the [complexity audit](https://claude.ai/artifact/8yMF8C6NBhtazEYJd4ngyP)): `forge queue` keeps one build at a time, in order, under a memory and CPU cap ([builds.md](builds.md#the-build-queue)). Bug fixes only; no new scheduling, priorities, dashboards or remote features.
6. **Every app is reachable from Device Lab** (decided 2026-11-15, replacing the same day's "joining is optional"). Each app goes on by cable (over-the-air installs don't work with a free team), and a free team allows three installed apps, so Device Lab is the one app on the phone that gets tested and hosts every example app on its Apps screen ([AGENTS.md](../AGENTS.md#device-tools)). `scripts/check-app-parity.sh` checks it. An app is published on its own only when the person asks.

The freeze ends when all four of these exit criteria are met, each checked on a physical iPhone and reported by a person:

| | Done when | Status (2026-11-15) |
|---|---|---|
| P1 ForgeUI 0.1 | The showcase's settings, list and create/edit form screens use no custom styling and run on an iPhone in light, dark and the largest text size | **Met** 2026-11-10 |
| P2 ForgeAppKit 0.1 | Pocket Library uses its alerts, destinations, load and submission state and form errors, and its UI tests that cover them pass on an iPhone | Not met: Pocket Library uses all of it (alerts, sheets, pushes with `forgeNavigationDestination`, the reader as a `forgeFullScreenCover`, load and submission state, field errors), and `uitests/` covers it; not yet recorded on an iPhone |
| P3 Rust foundations | Persistence guarantees ([#159](https://github.com/GeneralKnowledge/ixOS/pull/159)) are merged, Pocket Library runs on the `forge-app` `Runtime`, and its saved-version fixtures open in CI | Met on Linux: #159 merged 2026-11-11, Pocket Library on the `Runtime` since #176 (2026-11-13), and `store.rs` opens every `fixtures/library-v<N>.json` in the app crate tests. Not met on an iPhone: Pocket Library on the runtime not yet recorded in a sitting |
| P4 Reference app | Every requirement in Pocket Library's `requirements.toml` passes on an iPhone in one `forge ui run` sitting, with no app-local copies of framework code | Not met: every requirement, PL-1 to PL-10, has a device scenario in `uitests/` (run in Device Lab on test data), and the catalogue uses the `http` (URLSession) service; the sitting that runs them is next |

Whatever happens, the freeze is reviewed on 2026-12-15, so it can't drift into an indefinite one. At the review, it either ends, or gets a new date and the reason it still holds.

## Near term

### Autonomous dev loop (Forge M6) — Frozen

Frozen on 2026-11-11 ([above](#tooling-freeze-and-reuse-rules)): bug fixes only until P1 to P4 are done.


An agent takes a real task on an existing app, changes it, checks it on the iPhone, finds and fixes a real problem, and leaves one record of what ran and what didn't. Most of the pieces exist; M6 wires them together, aims exploration at what changed, correlates failures with logs and crashes, and demonstrates the loop on Good Dog. Plan: [forge-m6.md](https://github.com/GeneralKnowledge/forge-lab/blob/main/docs/forge-m6.md).

### iPhone acceptance of the integrated framework — Partly done

The native starter UI (#32), developer experience (#35), lifecycle (#36), native services (#34) and the Pocket Library acceptance app (#33) are integrated and green in CI together ([integration report](https://github.com/GeneralKnowledge/ixOS/blob/b5276a2a20cb5aa110d3cafa56bdc2596371a34c/docs/archive/pr-integration-report.md)). What is left is the physical-device run: [integration/iphone-acceptance.md](https://github.com/GeneralKnowledge/ixOS/blob/8fdd497cc51dfd0a8220f7dc013c3a25515f6c51/docs/integration/iphone-acceptance.md) is the single runbook, from a fresh `forge init` app to a Rust change reaching the phone. Pocket Library has run on an iPhone (UI test m0, 11/11, 2026-11-07) and now runs inside Device Lab, installed by cable (over-the-air installs are parked, [over-the-air.md](over-the-air.md)) ([status](status.md)). A fresh app from the current starter and the services check app have not been recorded on an iPhone; the services check's capabilities are the largest part of the "built, not yet run on an iPhone" list.

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

1. Migrate `examples/pocket-library-android` and `templates/android-app` together, in one PR, with the Android CI job green.
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

`forge-net` has a transport trait, test doubles, and a real blocking HTTPS client (`UreqClient`: ureq + rustls with the platform certificate verifier) behind its `ureq` feature. The iOS Weather example ([examples/weather](../examples/weather/README.md)) uses it end to end: Open-Meteo requests from Rust, typed errors, cancellation, and an offline cache. The Chat demo ([examples/relay/docs/chat-demo.md](../examples/relay/docs/chat-demo.md)) uses it on iOS (and on its frozen Android twin): a long-lived Server-Sent Events stream per room, reconnects with catch-up, and idempotent retries, against a local mock server.

HTTPS works on Android too (work in progress): hosts call `ForgeAndroid.init(applicationContext)` once, and certificates are checked against Android's trust store, with typed `Certificate` errors ([android-setup.md](android-setup.md#https-certificate-verification)). It is covered by Rust tests against local TLS servers and the JVM test of the JNI entry point; a run on a physical Android phone is still to be recorded.

Ordinary requests can be interrupted in flight: `HttpClient::send_cancellable` closes the connection, and the starter app crate wires it to Swift task cancellation and Kotlin coroutine cancellation ([app-crate.md](app-crate.md#lifecycle)). Apps also report the OS lifecycle to Rust (`set_phase`), which stops on-screen requests in the background and announces resume; device runs of it are not yet recorded. The Weather example still discards a late response instead of interrupting it. Still open: platform reachability.

**Moving iOS to URLSession** ([native-capability-roadmap.md](native-capability-roadmap.md#where-current-code-departs-from-native-first), decided 2026-11-10). Background downloads come first and are built: the `transfers` service runs them in a background `URLSession` while Rust keeps the ledger, checks each file and recovers after a relaunch ([native-services.md](native-services.md#background-transfers-transfers)); most of their phone checks passed on 2026-11-11. Ordinary requests went next, as the `http` service: a `URLSession` client behind `HttpClient`, opt-in, first used by Commonplace's feed fetches (passed on a phone). Streams (Chat's live event stream) now go through it too. Last comes `URLSession` by default on iOS. ureq stays for Linux tests and Android.

### iOS Simulator support — Open

Builds are **device-only** today. The Rust library ships as a single `aarch64-apple-ios` static library ([ADR 001](adr/001-ios-static-linking.md)), and the iOS Simulator only runs on macOS, so in the Linux-first workflow a physical device is the only way to run the app anyway. Simulator support mainly helps Mac users and macOS CI. It needs:

1. An `aarch64-apple-ios-sim` (and optionally `x86_64-apple-ios`) slice.
2. Packaging that can hold more than one slice: an XCFramework or a SwiftPM artifact bundle (ADR 001's fallback).
3. `ios_simulator = true` in `forge.toml` wired through the CLI (config parsing exists; the build path does not).
4. A macOS CI job that runs the XCTest suite on a simulator.

Good first project for someone with a Mac.

### Native iOS services (M3) — In progress

`crates/forge-services` gives Rust apps Keychain secrets, app directories and file import, local notifications, a single permission interface, the share sheet, and deep links. Each is a small Rust interface with a Swift implementation that Forge ships, and every `forge init` app has them installed ([native-services.md](native-services.md)). They are covered by Rust tests, macOS XCTest and an unsigned iOS build in CI. An iPhone run of [the device check](native-services-device-check.md) is still to be recorded. Built since: the web view, the native reader, the photos picker, background tasks, background transfers and the URLSession `http` service ([status](status.md)); camera capture is not built. The core services now also ship in ForgeKit as `ForgeNative` (`forge-native`), used by the starter and Good Dog; the other apps still copy them through `native_services!`, and moving everything to one route is phase 4 of the everyday capabilities plan below. Android implementations of the same interfaces wait until Android work resumes ([above](#android--work-in-progress)).

### Store distribution (App Store, Google Play) — Open

Not in this release. Today the CLI produces development builds (an xtool debug build signed for your own device on iOS, a debug APK for Android; see [builds.md](builds.md)). Release distribution needs:

- **Android:** release build type, signing config read from the environment (never committed), an `.aab` bundle, and R8/minification rules that keep JNA and the generated bindings.
- **iOS:** an archive and export path. App Store submission needs a distribution certificate and provisioning profile; whether xtool can produce a submission-ready build on Linux has not been verified, so the first deliverable is a documented macOS `xcodebuild archive` path.
- A `docs/distribution.md` covering both, plus release notes in [builds.md](builds.md#releasing-forge-itself).

The staged plan for the first real iOS release (Pocket Library; what Linux can do and what needs a Mac; `forge release check`) is [first-app-store-release.md](first-app-store-release.md).

### Any app on the iPhone (growing UI testing) — Frozen

Frozen on 2026-11-11 ([above](#tooling-freeze-and-reuse-rules)); the spikes below wait.


Forge UI Testing's routes (WebDriverAgent, Apple's accessibility service, `idevice` device services) used on apps Forge didn't build: observe, operate, and, the bigger goal, explore and verify with evidence. Not a separate stack: the same `Driver`, `UiNode` and `Locator`, with a person-written per-app policy for anything outside the project. Plan, safety model and an evaluation of iCTRL: [research/general-device-control.md](research/general-device-control.md). After the current milestone; four no-install spikes go first.

## Future

### Split Forge into focused projects — Future

Revived 2026-11-16 as four areas, framework, tools, infrastructure and apps, with ownership split before anything moves. [areas.md](areas.md) has the map, the dependency rule and the plan: boundaries and a check first, then the listed crossings fixed, then `examples/` renamed to `apps/`. Separate repositories wait until Forge Tools has its own release cycle. The earlier five-project proposal is in [Forge product boundaries](https://claude.ai/artifact/YBwLCg9UewZtk5cfkYwNNY).

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
