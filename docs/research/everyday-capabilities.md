# Forge everyday capabilities: audit and plan

Research and planning only, written 2026-11-16 against main `451a2af` for Reuben's "Forge everyday capabilities" review. No code was changed for it. It proposes; [roadmap.md](../roadmap.md) decides.

Revised the same day after Reuben's review: the backup analysis now says what is really backed up today, Phase 1 requires a typed open failure, Phase 3 separates background transfers from durable operations and retry, and Phase 4 starts with an investigation.

Forge has the right primitives and, since the forge-app runtime, a real application core; what it lacks is a handful of complete everyday workflows that every app currently finishes by hand or skips. The fix is not new layers. It is about six small, app-proven completions inside the existing crates and ForgeKit products, each pulled by a feature an app already wants.

Evidence base: main at `451a2af` (2026-11-16), every crate under `crates/`, `forgekit/Sources`, the starter templates, the ten apps under `examples/`, PRs #242 to #271, the friction log (A1 to A35), app-parity, device-validation and the ledger. Nothing was built or run for this review. "Verified" below means a person recorded it on the iPhone; everything else is labelled as Linux-tested, built only, or assumption.

## Part 1: Executive assessment

**What Forge does well (verified or strongly evidenced)**

- **The application core is solid.** `forge-app`'s `Runtime` gives every app one serial command thread, commit-then-publish, published snapshots, work scopes and orderly shutdown. Ten apps run on it via `ForgeHost` (app-parity: every `forge-host` cell is Yes). This closed foundation-review finding R1.
- **Persistence mechanics are unusually careful.** Atomic writes with directory `fsync`, versioned JSON with migrations and kept old versions, SQLite with migrations, FTS5, recovery only on real damage, a shared read connection (#260), staged file commits, hashed file stores, backup archives. All tested on Linux under `SIGKILL`.
- **Errors have a shared vocabulary.** `ErrorKind` (contract 3) now has `Network` and `Refused`; `ForgeFailure` maps kinds to words and retryability; parity rules `errors-shown` and `alerts-on-top` stop the bugs found today (A33, A34) coming back.
- **The quality loop works.** Requirements with tests, headless twin tests, `uitests/*.toml`, the parity check, the boundaries check and the ledger make regressions visible on Linux.
- **Native-first is mostly followed.** URLSession `http` service, background `transfers`, ImageIO, ShareLink, Swift Charts (FieldKit), MapKit, PencilKit and AVPlayer are used as Apple ships them.

**Where it is incomplete**

1. **Recovery flows stop at the error kind.** `Corrupt` and `NewerData` exist, but `ForgeRoot` only offers Try Again (`ForgeRoot.swift`, 38 lines). Pocket Library wrote its own Start Fresh; Sketchbook, Soundscape and Relay have none (A15). The data is safe; the user is stuck.
2. **Permissions are a status enum, not a lifecycle.** At least six apps ask for a protected resource (Weather and FieldKit location, Soundscape microphone, Sketchbook Photos add-only, Relay camera and local network, Chat photos and microphone). Each asks its own way; no shared denied-state view or Settings link; no re-check on resume (review P2, still open).
3. **No backup or protection policy for app data that isn't a cache.** `ForgeEnvironment.standard(app:)` already puts data in Application Support (backed up) and gives a real Caches folder (never backed up, may be purged), so caches are not the problem. What is missing is a way to keep re-downloadable files that must not be purged (offline songs, next) out of backups: nothing sets `isExcludedFromBackup` or chooses a `FileProtectionType` (searched `crates`, `forgekit`, every app), and every file keeps the protection class iOS applies by default. Review S5, open since 2026-11-08.
4. **Two ways to install native services.** The starter and Good Dog use `forge-native` (Swift ships in ForgeKit's ForgeNative); the other eight apps use the `native_services!` macro, which copies adapters into `Generated/`. Optional adapters (http, location, transfers, background, audio) exist only as copied files, so the published ForgeKit package alone cannot give an outside app URLSession or location.
5. **Localisation is tooling without users.** `forge` compiles String Catalogs on Linux, but no app ships a catalog, ForgeKit's own strings (`ForgeFailure`, `ForgeRoot`) are plain `String`s, and user-facing text produced in Rust bypasses catalogs entirely.
6. **Background and offline work is built, barely used.** The `transfers` service is used only by Device Lab; `connectivity` only by Device Lab; Chat's outbox is still the only durable operation queue; no retry policy beyond `Backoff` (review N2).

**Biggest opportunities**, each with a waiting app feature: an open-failure recovery screen (Sketchbook, Soundscape, Relay); a permission-denied pattern (FieldKit background location, Soundscape and Relay); a backup-excluded storage class for re-downloadable files (Soundscape offline downloads); one native-services route so ForgeKit works outside ixOS; and Soundscape's offline downloads as the first real user of background transfers.

## Part 2: Current architecture map

The actual architecture matches `docs/areas.md` in its dependency direction (the framework depends on no tool or app; `scripts/check-boundaries.sh` enforces it), but the framework itself is now nine Rust crates and five Swift products, more than `architecture.md` describes.

**Runtime path of one app (as built):** SwiftUI views (ForgeUI, ForgeAppKit) → `@Observable` model (Foundation only) → `ForgeHost` and `ForgeLifecycle` (ForgeKit) → generated bindings (app module, `ForgeApp`, `ForgeNative`) → the app's Rust crate → `forge-app` Runtime → `forge-storage`, `forge-net`, `forge-services`, `forge-reader`, `forge-runtime`. Native services call back from Rust into Swift adapters through UniFFI foreign traits.

### Framework

| Part | Language | What it really provides | Used by |
| --- | --- | --- | --- |
| `forge-runtime` | Rust (+ `ForgeLog.swift`) | `background`, `EventHub`, `Lifecycle` scopes, `app_shell!`, logging pipeline, `time` (ISO 8601, civil days), `now_ms` | Every app |
| `forge-app` | Rust | `Runtime`: command thread, snapshots, work scopes (`foreground`, `finishing`, `lifetime`), launch report with unfinished-work marker, `Document`, field validation, `ErrorKind` contract | Every app except Device Lab (by decision) |
| `forge-storage` | Rust | `write_atomic`, `JsonFile`, `Database` (SQLite, FTS5, `reader`), `FileStore` (staging, hashing, sweep), `backup` (zip archives), `csv` | Every app with data; `backup` only Pocket Library |
| `forge-net` | Rust | `HttpClient` trait, `UreqClient`, streams, `CancelHandle`, `Backoff`, `download_to_file`, test doubles | Chat, Commonplace, Pocket Library, Soundscape, Weather |
| `forge-native` | Rust → ForgeNative | Core services as one UniFFI component: Keychain, directories, notifications, permissions, sharing, deep links | Starter, Good Dog |
| `forge-services` | Rust + Swift adapters | Same core set via `native_services!` (copied), plus optional `http`, `transfers`, `background`, `location`, `motion`, `connectivity`, `device_status`, `bluetooth`, `audio`, `biometrics`, `ai`; Swift helpers `ForgeWeb`, `ForgeImages`, `ForgeDocuments`, `ForgeAudioSession` | Eight apps and Device Lab |
| `forge-reader` | Rust + Swift | Styled blocks, HTML to blocks, anchors, reading view | Pocket Library, Commonplace |
| ForgeKit (`ForgeHost`, `ForgeRoot`, `ForgeLifecycle`) | Swift | Opens the core off the main thread, reports phases, holds background time, event loop | Every app |
| ForgeUI | Swift, no Rust | Tokens, themes, button and field styles, rows, cards, badges, state view, launch appearance | Every app |
| ForgeAppKit | Swift, on ForgeUI | `ForgeAlert`, destinations, `ForgeLoad`, `ForgeSubmission`, field errors, `forgeSaveOnLeave`, `ForgeFailure` | Every app |

### Tools, infrastructure and apps

- **Tools:** `forge` CLI (build, run, bindings, config, check, ledger, verify, diagnose, queue), `forge-device`, `forge-uitest`, `forge-crash`, `forge-devproto`, templates, Device Lab. forge-lab (MCP, explore, run records) is outside ixOS.
- **Infrastructure:** CI workflows, `forgekit.sh` releases (ForgeKit 0.6.0, contract 3), toolchain pins, the laptop build queue.
- **Apps:** Chat, Checklist, Commonplace, FieldKit, Good Dog, Pocket Library, Relay, Sketchbook, Soundscape, Weather, each with its own Rust crate, hosted together in Device Lab.

### Drift found between documents and code

| Where | Says | Actually |
| --- | --- | --- |
| `areas.md`, `boundaries.toml` | Relay's channel moves into forge-net in wave 4 | Roadmap (2026-11-16) excludes it: iroh replaces the wire (#267). The crossing's plan should say "removed when Relay drops forge-devproto" |
| `architecture.md` crates table | Lists runtime, services, storage, net, chat-core, cli; says Chat, Good Dog, Weather "move to it in wave 3" | Missing `forge-app`, `forge-native`, `forge-reader`; the wave 3 moves are done; `chat-core` is an app crate, not framework |
| `architecture.md` layers | "Forge Device (planned)" | Built and partly verified |
| `roadmap.md` near term | Pocket Library "installed over the air" | OTA is parked; install is by cable |
| `roadmap.md` M3 | "Next: the web view, documents, the native reader, photos and camera, background tasks" | Web view, reader, background tasks and photos picker are built |
| `foundation-review.md` status line | Only S1, S2, S6, N3, R2, P3 fixed | R1 is fixed by `forge-app`; R4 partly (finishing scope, launch report); N4 built (`transfers`); S4 and S7 partly |
| `native-services.md` vs code | Both routes described as current | Only Good Dog and the starter use `forge-native`; no parity rule tracks the move |
| `native-capability-roadmap.md` | Platform limits move from Rust to adapters | `notify.rs` still hard-codes `MIN_REPEAT_SECONDS = 60` |

## Part 3: Everyday capabilities audit

Most of the 39 capabilities checked are either complete enough to leave alone or rightly app-owned or Apple-only; five need work now (marked High), and a handful are absent by design or for lack of a paid team. Device evidence follows `status.md`: **Tested** means a person recorded it on the iPhone, **Partly** means some of it was seen, **Untested** means built and Linux-tested only.

App abbreviations: CH Chat, CL Checklist, CP Commonplace, FK FieldKit, GD Good Dog, PL Pocket Library, RL Relay, SK Sketchbook, SS Soundscape, WE Weather, DL Device Lab.

### Evidence matrix

| Capability | Consumers | Current implementation | Existing Forge support | Completeness | Device evidence | Duplication | Pain points | Ownership | Recommendation | Priority | Dependencies |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| **A. Lifecycle** Foreground, background, shutdown | All | `ForgeHost` + `ForgeLifecycle`; `Runtime` phases and scopes | forge-app, ForgeKit | Complete | Partly (GD relaunch, app switching in DL) | None since wave 3 | Chat needed its own lifecycle thread (A31) | Forge | Keep; add `on_transition` hook when Relay needs it | Low | Relay M2 |
| Must-finish work and unfinished-run report | CL, FK, PL, RL, SS use `finishing`; CH, CP, GD, PL read the launch report | `Runtime::finishing`, `.forge-app.unfinished` marker, `ForgeBackgroundTime` | forge-app, ForgeKit | Partial: no app shows the user what was interrupted in a shared way | Untested | Each reader words it itself | Report is a bool, not what was lost | Forge + App | Improve: one ForgeAppKit notice for "didn't finish last time" once two apps word it the same | Medium | None |
| State restoration (screen, scroll, selection) | None today | Nothing; no `@SceneStorage` or `NSUserActivity` anywhere | None | Absent | n/a | n/a | Relaunch always lands on the root; tabs remembered by `@AppStorage` in DL only | Apple API + App | Reject a framework layer; document `@SceneStorage` in ios-ui.md | Low | None |
| Recovery after unexpected termination (data) | All | Atomic writes, migrations, `Corrupt`/`NewerData` kinds, `save_over_corrupt`, kept old versions | forge-storage, forge-app | Mechanics complete; **user flow absent** (`ForgeRoot`: Try Again only) | Untested (no forced-corruption run) | PL has its own Start Fresh; SK, SS, RL have none (A15) | A user with unreadable data has no way forward | Forge (ForgeKit) | **Improve**: open-failure screen by `ErrorKind` | **High** | None |
| **B. Data** Persistence, transactions, migrations | All | JSON `Document` or SQLite `Database` | forge-storage | Complete (mechanics) | Tested: JSON snapshot (Chat); SQLite untested | None | No Rust data-migration hook for SQLite (S7) | Forge | Keep; add the hook with the first SQLite data migration | Low | A real migration |
| Import and export | CL (CSV), FK (GPX, GeoJSON), PL (CSV, backup), SK (PNG, PDF), SS, RL (Files), WE | Per app, through `ShareLink` and `fileImporter` | `csv`, `ForgeDocuments`, `ForgeShareLink`, `FileStore` staging | Partial | Untested (CSV, backup) | Formats are app-specific, as they should be | None severe | App (formats) + Apple (sheets) | Keep app-owned | Low | None |
| Backup and restore | PL | `forge_storage::backup` zip with manifest and hashes | forge-storage `backup` | Complete for one app | Untested | None | Only one user | Forge | Keep; second user only when an app asks (FieldKit trips are a candidate) | Low | None |
| Backup exclusion and file protection classes | SS offline songs (next); any later re-downloadable data kept in Application Support | `ForgeEnvironment` separates Application Support (backed up) from Caches (not backed up, purgeable); nothing sets exclusion or an explicit protection class | `ForgeEnvironment`, `AppDirectories` | **Absent** for the middle case: kept, but not backed up | n/a | n/a | No current file is wrongly backed up (verified: Chat, Good Dog and Pocket Library keep no caches; Commonplace's articles are in its database; no app stores downloaded AI models yet). The gap appears with Soundscape's offline songs. Files use iOS's default protection class (S5) | Forge | **Improve**: one backup-excluded class, applied to a folder so atomic replaces keep it | **High** | SS offline downloads |
| Data deletion (erase everything) | PL, GD (reset) | App code | `FileStore::sweep`, Keychain | Partial | Untested | Two apps, different meaning | Keychain items outlive the app's data | App + Forge (Keychain reconcile) | Defer the UI; fix Keychain reconcile with storage classes | Low | Storage classes |
| Conflict handling, sync | CH | Chat outbox, server ids | None (by design) | App | Partly (Chat) | None | None | App | Reject a sync engine | n/a | n/a |
| **C. Permissions** Status and request | CH, FK, WE, SS, SK, RL, GD (notifications) | `PermissionProvider` + adapters (notifications, location, mic); SK, SS, RL ask Apple directly | forge-native, forge-services | Primitive | Partly (Weather location fix) | 3 apps ask directly | No denied view, no Settings link (only DL has one), no re-check on resume (P2) | ForgeAppKit (view) + Apple | **Improve**: one SwiftUI denied-state pattern, re-check on `resumed` | **High** | FK background location, SS microphone |
| Local network, Bonjour | RL | `forge.toml` `bonjour_services`, deprecated `NetService` | config only | Partial | Partly (Relay same Wi-Fi) | None | A27 | App + Apple | Keep app-owned | Low | iroh rework |
| **D. Accessibility** Labels, identifiers, Dynamic Type | All | ForgeUI styles use `@ScaledMetric`, 44 pt minimum, Reduce Motion, combined elements; parity rules | ForgeUI, `forge ui audit`, `-ax5` baselines | Complete in components; apps checked by rules | Partly (showcase light/dark; largest text not on phone) | None | Charts and canvases (FK, SK) have no a11y summary | Forge + App | Keep; add one largest-text phone pass per app | Medium | Phone sitting |
| Increased Contrast, RTL, keyboard | All | `ForgeContrast` token; no RTL or keyboard work | ForgeUI tokens | Partial | Untested | None | No RTL language in any app | Apple + ForgeUI | Defer until an app is localised | Low | Localisation |
| Localisation | None | Linux String Catalog compiler | `forge config sync` | **Tooling only** | Untested | n/a | ForgeKit strings are plain `String`; Rust error text is English copy | Forge + App | **Improve** when the first app localises: ForgeKit strings as `LocalizedStringResource`; Rust returns kinds, Swift words | Medium | Release decision |
| **E. Preferences** | CP, PL, WE, CH, SS | Domain settings in Rust documents; display prefs in `@AppStorage` | `Document`, `JsonFile` | Complete (by convention) | Partly (CP settings) | Small, idiomatic | No written rule on which goes where | App + Apple | Keep; write the rule into app-crate.md | Low | None |
| **F. Observability** Logging | All | Rust `logging`; Swift `ForgeLog` used by 7 apps; `print(` in CL, PL, RL | forge-runtime | Partial | Partly (Warn seen; Info recheck pending) | n/a | Swift `print` bypasses the pipeline; deep-link adapter prints URLs (P4) | Forge | Improve: parity rule "no print", recheck Info on phone | Medium | None |
| Crash diagnostics | All | `forge device crashes`, `forge crash`, `forge diagnose` | Tools | Complete for developers | Tested | None | Nothing for a user to send a report | Tools | Keep | n/a | n/a |
| In-app diagnostic export (user to developer) | None | Nothing; no MetricKit | Recent-records ring in `logging` | Absent | n/a | n/a | Remote failures only diagnosable with the cable | Forge + Apple (MetricKit) | Defer until first App Store app | Low | App Store release |
| **G. External events** Deep links, notification taps | CH, CP, FK, GD | `DeepLink` service, notification inbox, `onOpenURL` | forge-native, forge-services | Complete (primitive is enough) | Untested | Each app routes to its own screens (correct) | Hosted apps in DL share one scheme; URL printed in log | App owns routes; Forge parses | Keep; redact the log line | Low | None |
| Incoming share content, document open-in | RL (Files app, #268) | App | None | Absent (share extension not built) | Untested | n/a | xtool extension template missing | Tools + App | Defer to an app that needs it (Relay) | Low | Extension support |
| Universal links, auth callbacks | None | Nothing | None | Absent | n/a | n/a | Universal links need a paid team | Apple | Reject for now | n/a | Paid team |
| **H. Networking** Requests, streams | CH, CP, PL, SS, WE | URLSession `http` service behind `HttpClient`; ureq on Linux | forge-net, forge-services `http` | Complete | Partly (CP, SS over Tailscale; Chat stream untested) | None | Whole core service set needed for one `http` (A35) | Forge | Keep; single route (Part 5) | Medium | Native-services route |
| Retry, offline, connectivity | CH (outbox), CP (stale refresh), WE (cache) | `Backoff`; `connectivity` service used by DL only | forge-net, forge-services | Primitive | Untested | Three apps each decide when to retry | No retry policy, no `Retry-After` (N2) | Forge (policy) + App (when) | Improve with the second durable operation | Medium | SS downloads or RL |
| Background transfers | DL only | `transfers` service, Rust ledger, recovery | forge-services `transfers` | Complete, **no app user** | Partly (DL tests 1 to 3, 6) | n/a | Unproven in a real feature | Forge | **Prove** with SS offline downloads | **High** | SS PR 3 |
| **I. Files** Import, share, preview | CH, CL, PL, RL, SK, every exporter | `fileImporter`, `ForgeShareLink`, `.quickLookPreview`, `FileStore` | `ForgeDocuments`, `ForgeShareLink`, `FileStore` | Complete for current apps | Partly (FK export, RL Quick Look) | Quick Look in 3 apps (one line each) | None | Apple + Forge | Keep; no wrapper for Quick Look | Low | None |
| Photo picking | CH, next FK, SS artwork | `PhotosPicker` + `ForgeImages` | `ForgeImages` | Partial | Untested | Second and third uses coming | Item to staged file is hand-written | ForgeNative/helpers | Extract after FK photo notes passes (planned) | Medium | FK feature |
| **J. Background work** Refresh tasks | CH, CP | `background` service (BGTaskScheduler) | forge-services | Partial | Untested | None | iOS decides timing; untested on phone | Forge + Apple | Prove with CP background refresh | Medium | CP feed reader |
| Local notifications | GD (reminders), CH | `LocalNotifier`, grouping, actions | forge-native | Partial | Untested | None yet (CL is the planned second) | 60 s limit in Rust | Forge | Keep; move the limit to the adapter | Medium | CL reminders |
| **K. Security** Keychain | CH (and core set in all) | `SecureStore` | forge-native | Complete | Untested | None | No Linux stand-in (A17) | Forge | Keep; file stand-in when RL needs it | Low | RL |
| Face ID lock | CH (service listed) | `biometrics` service | forge-services | Partial | Untested | None | No second user | Forge | Defer | Low | None |
| **L. Testing** Rust units, twin tests, UI tests | All | `cargo test`, headless twin, `uitests/*.toml`, `forge ui run` | forge-uitest, `FixedPermissions`, `RecordingNotifier`, `StaticClient` | Complete on Linux; device runs partial | Tested (UI test m0) | None | No shared test clock: apps call `now_ms()` or pass `now` by hand | Forge | Small: document "pass `now`" as the rule | Low | None |
| Swift compilation without a Mac | All | `check-ios-xtool.sh` on the laptop | Tools | Complete | n/a | n/a | Swift merged uncompiled (A28) | Tools | Keep the rule: compile before merge | Medium | None |
| **Beyond A-L** Search | CP (FTS5), PL (next), CH, CL, GD, SS, WE (`.searchable`) | FTS5 + `Database::reader` | forge-storage | Partial | Untested (CP search) | CP and PL next | None yet | Forge | Extract after PL search (planned) | Medium | PL search |
| Empty, loading, error states | All | `ForgeStateView`, `ForgeLoad`, `ContentUnavailableView` | ForgeUI, ForgeAppKit | Complete | Partly | None | None | Forge | Keep | n/a | n/a |
| Media playback and Now Playing | PL (read aloud), SS (music) | Hand-written `MPNowPlayingInfoCenter` and remote commands in both; `ForgeAudioSession` lacks a music mode | `audio` service, `ForgeAudioSession` | Partial | Partly (SS background audio, PL read aloud) | **Two apps, same plumbing** | A35 | Forge helper (Swift only) | **Extract** a Now Playing helper + `.music` session | Medium | SS player merged |
| Haptics | 7 apps | `sensoryFeedback` | SwiftUI | Complete (Apple) | Untested | Fine | None | Apple | Reject a wrapper | n/a | n/a |
| Charts | FK, next GD, WE | Swift Charts; shaping in Rust | None | App | Partly (FK) | Possible series thinning | None | Apple + App | Extract shaping only if GD repeats it (planned) | Low | GD progress |
| In-app purchases, review prompt, update awareness | None | Nothing | None | Absent | n/a | n/a | Paid team | Apple + App | Defer to App Store work | n/a | Paid team |

### What the apps show

| App | Main capabilities | Uses from Forge | Apple directly | Reusable or duplicated | Keep in the app |
| --- | --- | --- | --- | --- | --- |
| Chat | Rooms over SSE, offline outbox, attachments, on-device assistant, voice notes | Runtime, SQLite, http, ai, audio, biometrics, background, LocalNotifier | PhotosUI, QuickLook, ImageIO | Outbox is the only durable operation queue; photo picking (first user) | Message states, rooms, assistant persona |
| Checklist | Items, search, CSV | Runtime, csv, finishing | `fileImporter` | Will be second user of notifications and recurrence | Task rules |
| Commonplace | Feeds, reader, FTS search, settings | Runtime, SQLite + reader, http, background, forge-reader | (none beyond SwiftUI) | Conditional GET written in the app; first search user | Feed rules, retention |
| FieldKit | Trips, map, charts, GPX/GeoJSON, background location | Runtime, SQLite, hashed files, location | MapKit, Charts | Distance maths in forge-services (A4); next photo-picker user | Track maths, split rules |
| Good Dog | Training plans, reminders, progress | Runtime, forge-native (only app on it), LocalNotifier | (none beyond SwiftUI) | Time-zone day logic (also PL goals) | Training safety rules |
| Pocket Library | Books, EPUB reader, read aloud, catalogue, backup, AI summaries | Runtime, csv, backup, http, ai, forge-reader | MediaPlayer, UIKit | Now Playing (shared with SS); own Start Fresh | Reading rules, catalogue mapping |
| Relay | Word-code transfer (iroh), QR, Bonjour, Files | Runtime, hashed files, ai, forge-devproto (crossing) | Network, Vision, VisionKit, QuickLook | Camera and local-network permission asked directly | Pairing levels, transfer protocol |
| Sketchbook | PencilKit drawing, layers, export | Runtime, hashed files | PencilKit, Photos, QuickLook | Photos add-only permission asked directly; no Start Fresh | Undo model (A23) |
| Soundscape | Music player, library, Listen picks | Runtime, hashed files, http | AVFoundation, MediaPlayer | Now Playing (shared with PL); music session mode; offline downloads will need transfers | Takes, ratings, blind mode |
| Weather | Forecasts, places, offline cache | Runtime, http, location | (none beyond SwiftUI) | `LocationProviding` is a test seam over the `location` service, not its own Core Location code | Forecast rules |

No app shows unnecessary platform plumbing beyond the items above; most Swift files are screens. The heaviest avoidable plumbing is the core native-services set and `AppServices.swift` an app must take on just to use `http` (A35).

## Part 4: Missing capabilities, ranked

Six gaps are worth closing; each already has real consumers and an app feature that can prove it. They are ranked by user harm first, then consumer count.

### Critical foundational gaps

1. **Opening saved data that can't be read.** *Evidence:* `ForgeRoot.swift` shows Try Again for every open failure; `ErrorKind::Corrupt` says "offer recovery" and `NewerData` says "never offer a reset", but nothing in ForgeKit acts on either. Pocket Library built its own Start Fresh (`SettingsView.swift`, `LibraryModel.swift`); friction log A15 lists Sketchbook, Soundscape and Relay without one. *Harm:* a user whose file is damaged, or who restored a newer backup, is stuck in a retry loop. *Consumers:* every app on `ForgeHost` (ten).
2. **Re-downloadable files that must stay but not be backed up.** *Evidence:* `ForgeEnvironment.standard(app:)` gives Application Support (backed up by iOS) and Caches (not backed up, and purgeable under storage pressure); no `isExcludedFromBackup` or explicit `FileProtectionType` anywhere in the repository; foundation review S5 open since 2026-11-08. Checked today: Chat, Good Dog and Pocket Library pass their data folder as the caches folder and keep no caches; Commonplace's articles live in its database (user data); no app stores downloaded AI models yet; Soundscape's Listen streams and stores nothing. So **nothing is wrongly backed up today.** *Harm when it arrives:* offline songs can't go in Caches (iOS may delete them when space is low, losing the person's offline music), and in Application Support they would be backed up. Apple's guidance is to exclude them, and ordinary file operations (a replace or an atomic rename) can drop the per-file flag, so it has to be set on the folder or reapplied after each write. *Consumers:* Soundscape first; any later re-downloadable store (a model download, a feed's offline images) second.

### High-value shared capabilities

3. **The permission lifecycle.** *Evidence:* six apps need a protected resource; Sketchbook (`ExportSheet.swift:82`), Soundscape (`LoopEngine.swift:225`) and Relay (VisionKit) ask Apple directly; only Device Lab links to Settings. Review P2 (no re-check on resume) is open. *Harm:* a refused permission leaves a blank or broken screen, which the roadmap's own completeness rule forbids ("a useful screen when they are refused"). *Consumers:* FieldKit, Weather, Sketchbook, Relay, Chat, Good Dog (Soundscape's microphone use leaves with the Studio removal).
4. **One route for native services.** *Evidence:* `forge-native` (starter, Good Dog) and `native_services!` (eight apps) coexist; optional adapters are only copied from the checkout by `scripts/generate-bindings.sh`, so the released ForgeKit can't give an outside app `http`, `location` or `transfers`. Soundscape had to take nine generated files and an `AppServices.swift` to make one URLSession request (A35). *Harm:* developer experience outside ixOS, and two code paths to maintain. *Consumers:* every app and every external user.
5. **A real user of background transfers.** *Evidence:* `transfers` and `connectivity` are used by Device Lab only. *Harm:* the code that matters most for offline reliability has never carried a real feature. *Consumers:* Soundscape offline downloads (planned PR 3). This is a separate question from durable operations and retry: Chat's outbox is still the only durable operation queue and `Backoff` has no attempt limit or `Retry-After` (N2), and Soundscape's downloads don't answer either (Phase 3 says what they do and don't prove).

### Quality improvements

6. **Now Playing and music audio.** *Evidence:* `MPNowPlayingInfoCenter` and `MPRemoteCommandCenter` hand-written in Pocket Library and Soundscape; `ForgeAudioSession.playback` uses `spokenAudio`, wrong for music (A35). Second use exists now.

Also missing but **not** ranked, because no app needs them yet or Apple already solves them: state restoration (`@SceneStorage` is enough), in-app diagnostic export (wait for the App Store app), share extension (wait for Relay), universal links and purchases (paid team).

## Part 5: Architectural recommendations

Every recommendation fits an existing crate or Swift product; none needs a new crate, package or layer. Of the seven unifying concepts the brief suggested, three are justified by evidence, two already exist, and two should be rejected.

### Shared concepts tested against the evidence

| Concept | Verdict | Apps that need it | What it replaces | Rust, Swift or both | Where it lives | Extract now? | Validated by |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Common error and recovery semantics | **Exists** (`ErrorKind`, `ForgeFailure`); missing piece is the open-failure flow | All ten | PL's own Start Fresh; Try Again loops elsewhere | Both: Rust says `Corrupt`/`NewerData` and keeps the set-aside copy; Swift shows choices | forge-app (an `open_fresh` that sets the old file aside) + ForgeKit `ForgeRoot` | **Yes**: platform-level, ten consumers, one existing app copy | Sketchbook opening a deliberately damaged file in Device Lab |
| Data lifecycle contract (storage classes) | **Justified, narrowly** | SS first; later re-downloadable stores | Nothing (gap) | Rust names the folder's class; the Swift adapter sets `isExcludedFromBackup` on that folder (so atomic replaces inside it keep the exclusion) and leaves protection at iOS's default unless an app asks | forge-storage `FileStore` option + ForgeNative directories adapter | **Yes**, with Soundscape offline downloads as first real user; it is platform-level (Apple's API, same for every app). Only three places: Application Support (backed up), Caches (already excluded by iOS), and one kept-but-excluded folder | Songs absent from the app's size in Settings › iCloud › Manage Storage; still present after the phone runs low on space |
| Capability and permission contract | **Justified, Swift-first** | FK, WE, SS, SK, RL, CH, GD | DL's Settings link; three direct Apple calls with no denied state | Swift. Rust only for permissions the core reads (location, notifications), which already exist | ForgeAppKit: a denied-state view with a Settings button; ForgeKit: re-check registered permissions on `resumed` | Partly: the view now (seven apps); the re-check after FieldKit background location shows a real change | FieldKit with location turned off in Settings, then on, without relaunch |
| Consistent operation lifecycle (durable, retryable work) | **Not proven yet; wait** | CH outbox; RL resume; SS downloads only for transfers | Chat's outbox later | Rust | forge-net (`RetryPolicy`) + forge-services `transfers` | No. Soundscape's downloads can prove a reliable transfer-specific recovery (the `transfers` ledger, `Failure::Interrupted`, `retry`), not a generic durable-operation or retry framework. That needs a second app with similar needs, compared with Chat's outbox (review N1) | A second durable queue, not Soundscape |
| Application diagnostics | **Exists for developers**; reject an in-app layer now | None yet | n/a | n/a | Tools | No | n/a |
| External event handling | **Exists** (`DeepLink`, notification inbox) | CH, CP, FK, GD | n/a | n/a | forge-native | No new work; redact the URL log (P4) | n/a |
| Testable system integrations | **Exists** (`FixedPermissions`, `RecordingNotifier`, `StaticClient`, headless twin) | All | n/a | n/a | forge-native, forge-net | No. A shared clock is not justified: passing `now` works and two apps already do | n/a |

### Where each kind of work belongs

**Framework, existing components:**

- *forge-app / ForgeKit:* open-failure recovery; a phase hook (`on_transition`) when Relay is the second user after Chat.
- *forge-storage:* storage classes; SQLite data-migration hook with the first real data migration; FTS helper after Pocket Library search.
- *forge-native and ForgeNative:* become the one route for native services, with optional services as Cargo features of `forge-native` and Swift adapters shipped in ForgeNative (compiled only when the app enables them). Retire the copying in `generate-bindings.sh` app by app, tracked as a parity rule.
- *ForgeAppKit:* permission denied-state view; a "last session didn't finish" notice once two apps word it alike.
- *forge-services helpers (Swift):* Now Playing helper and a `.music` audio session use; photo-item-to-staged-file after FieldKit photo notes.
- *forge-net:* `RetryPolicy` (attempt limit, deadline, `Retry-After`, retryable kinds) with the second durable operation.

**Native Apple integrations, no Forge wrapper:** state restoration (`@SceneStorage`), Quick Look, haptics (`sensoryFeedback`), charts (Swift Charts), photo picking UI (`PhotosPicker`), share sheet, MapKit, PencilKit, VisionKit scanning, App Store review prompts. Forge documents the pattern in ios-ui.md where an app needs one.

**Application-owned:** settings screens and their organisation; import and export formats; navigation routes for deep links; conflict and merge rules; what "erase all data" means; Bonjour and pairing; charts' data shaping until a second app repeats it; every domain rule listed in the app table in Part 3.

**Tools:** crash and log collection, diagnosis, UI tests and accessibility audits stay in `forge`; nothing from these moves into the framework.

## Part 6: Developer experience assessment

A developer on Linux can get from `forge init` to a Rust-backed, ForgeUI app on an iPhone, but four steps still need knowledge of ixOS internals: adding a native service, recovering from bad data, handling a refused permission, and shipping. Inside the repository the experience is good; with published ForgeKit alone it is incomplete.

| Step | Today | Main obstacle | Evidence |
| --- | --- | --- | --- |
| 1. Create an app | `forge init` (with `--library`), README with what to replace | Starter still carries example notes to delete by hand | A21, #242 |
| 2. Add Forge | Starter pins ForgeKit 0.6.0 and crates at `ixos_rev` | Optional Swift adapters come from the `forge` binary's own checkout, not from the ForgeKit release | `generate-bindings.sh:155-178`, `app_config.rs:2021` |
| 3. Rust domain logic | `Runtime`, `Document`, validation, `app_error_conversions!` | Small leftovers: `start_fresh` boilerplate, no default for UniFFI records in Swift | A15, A32 |
| 4. Native Apple functionality | Core set via forge-native; optional via macro | Two routes; one request type pulls in nine files (A35); platform limits still in Rust (`notify.rs`) | A35 |
| 5. Consistent interface | ForgeUI and ForgeAppKit, parity rules | No compile of SwiftUI on Linux; first look at charts is on the phone | A28, A32 |
| 6. Persist and migrate | Strong; fixtures per schema version | SQLite data migrations need hand-written Rust outside the migration list | S7 |
| 7. Permissions | `forge.toml` texts, `PermissionProvider` | No denied-state pattern; apps call Apple directly | Part 4 item 3 |
| 8. Errors and interruptions | `ErrorKind`, `ForgeFailure`, finishing scope, launch report | Open failure has no recovery screen | Part 4 item 1 |
| 9. Test | `cargo test`, twin tests, `uitests`, `forge check swift` | Device runs need the shared laptop and a free-team phone (3 apps, 7 days) | AGENTS.md |
| 10. Diagnose | `forge diagnose`, `forge device logs`, `forge crash` | Swift `print` still in three apps; Info logs not yet confirmed on phone | status.md |
| 11. Export and manage user data | `backup`, `csv`, `FileStore`, share sheet | No storage classes; no shared erase story | S5 |
| 12. Release | Not supported | Needs a paid team and, for now, a Mac | first-app-store-release.md |

**Outside the ixOS repository.** An app built only on the published ForgeKit gets ForgeApp, ForgeKit, ForgeNative (core services), ForgeUI and ForgeAppKit. It does not get URLSession networking, location, background tasks, transfers or the Swift helpers without the ixOS checkout that built `forge`. Consolidating on forge-native (Part 5) is the single change that fixes most of this.

**Documentation load.** A newcomer meets `architecture.md`, `app-crate.md`, `ffi-policy.md`, `native-services.md` and `native-capability-roadmap.md` before writing a feature, and they disagree in the places listed in Part 2. The fixes are in Part 10.

## Part 7: Prioritised roadmap

Six phases, one PR stream at a time, each carried by an app feature that is already wanted. They slot in after the current Soundscape and Relay PRs and alongside wave 5; none blocks app work, and each ends with a short Device Lab check (about 10 minutes).

| Phase | Objective | Proving app feature | Size |
| --- | --- | --- | --- |
| 0 | Docs tell the truth | none (docs only) | 1 small PR |
| 1 | Unreadable data has a way forward | Sketchbook, then Pocket Library moves onto it | 1 to 2 PRs |
| 2 | A refused permission never leaves a dead screen | FieldKit background location; Sketchbook save to Photos; Relay camera | 2 PRs |
| 3 | Offline songs are kept but not backed up; background transfers carry a real feature | Soundscape offline downloads (its planned PR 3) | Inside SS PR 3 + 1 framework PR |
| 4 | One way to add a native service, usable outside ixOS | Soundscape and Weather first, then the rest | 1 framework PR + 1 per app wave, ForgeKit 0.7 |
| 5 | Music and spoken audio share one Now Playing helper | Soundscape player, Pocket Library read-aloud | 1 PR |

### Phase 0: Documentation matches the code

- **Scope:** the drift rows in Part 2 and the edits in Part 10. No code.
- **Acceptance:** the markdown link check and `forge ledger generate` pass; every row of Part 2's drift table is resolved or noted.
- **Device:** none. **Excludes:** restructuring docs again.

### Phase 1: Recovery when saved data can't be opened

- **Objective:** `Corrupt` offers "Start Fresh" (keeping the unreadable file aside and saying where); `NewerData` says "update the app" and offers nothing destructive.
- **Design requirement:** the open contract must carry a typed failure classification across the Rust/Swift boundary, and `ForgeRoot` must never infer corrupt or newer data from an error message. Today `ForgeHost.start()` catches the error thrown by the app's `open` closure and stores `Status.failed(String)` from `ForgeErrors.describe(error)`. The kind is available at that moment when the thrown error conforms to `ForgeKindedError`, and is then thrown away. Investigate first, before any code: (a) whether each app's open error conforms to `ForgeKindedError` (the generated errors need the one-line conformance); (b) a `Status.failed` that keeps the `ErrorKind` (or a `ForgeFailure` built from it) beside the words, which changes ForgeKit's public API and needs a release; (c) what a non-kinded error maps to (`Internal`, so never a destructive offer); (d) where Start Fresh runs: a second open that sets the old file aside in Rust, not a Swift file deletion.
- **Scope:** `forge-app`: one documented way to open fresh with the old file set aside (generalising `save_over_corrupt` and the A15 `start_fresh` boilerplate). ForgeKit `ForgeRoot`: show the right screen by `ErrorKind`. Sketchbook adopts it; Pocket Library's own copy is deleted in a follow-up. Parity rule `open-recovery`.
- **Evidence apps:** Pocket Library (existing copy), Sketchbook, Soundscape, Relay (none).
- **Dependencies:** the investigation above. ForgeKit release for outside apps.
- **Acceptance:** Linux tests open a damaged and a newer fixture for each app on `Document` and assert the kind that reaches `ForgeHost.status`, not its words; twin test shows both screens; a plain Swift error never offers Start Fresh; parity check names apps without it.
- **Device:** in Device Lab, a debug launch option that corrupts Sketchbook's file; Start Fresh works and the old file is still in the container (seen with `forge device` or Files). One person-recorded row.
- **Excludes:** automatic repair, sending the damaged file anywhere.

### Phase 2: The permission lifecycle

- **Objective:** every protected feature has three states the user can act on: ask, allowed, refused with a Settings button and a sentence why.
- **Scope:** ForgeAppKit: a denied-state view built on `ForgeStateView` with `UIApplication.openSettingsURLString` (Swift only). ForgeKit: re-check registered permissions on `resumed` and publish the change (review P2). FieldKit's background location adopts it; then Sketchbook (Photos add-only) and Relay (camera).
- **Evidence apps:** FieldKit, Weather, Sketchbook, Relay, Chat, Good Dog. (Soundscape's microphone use goes away with the Studio removal, so it is not counted.)
- **Dependencies:** none for the view; the re-check waits until FieldKit shows a status change without relaunch on the phone.
- **Acceptance:** each adopting app has a requirement and UI scenario for the refused state; `forge ui` permission reset drives it.
- **Device:** turn FieldKit's location off in Settings, return, see the refused state, tap Settings, turn it on, return, recording available. Confirms which iOS permission changes terminate the app.
- **Excludes:** a Rust permission abstraction for UI-only permissions; a generic onboarding flow; wrapping each Apple permission API.

### Phase 3: Storage classes and background transfers, proven by offline songs

- **Objective:** downloads continue while the app is suspended or ended by iOS; after a force quit, the next launch finds the unfinished ones and starts them again; songs are kept when the phone is low on space and excluded from backup.
- **iOS behaviour this must respect:** when iOS ends the app, the background session keeps running and iOS relaunches the app to reconnect it. When the person force-quits from the app switcher, iOS cancels every background transfer and does not relaunch the app. Recovery after a force quit is therefore the app's own: on the next manual launch, the `transfers` ledger reports the task as `Failure::Interrupted` (already built) and Soundscape calls `retry`, which schedules a new transfer. Nothing resumes the original task. Primary sources (Apple): [`background(withIdentifier:)`](https://developer.apple.com/documentation/foundation/urlsessionconfiguration/background(withidentifier:)) ("If the user terminates the app from the multitasking screen, the system cancels all of the session's background transfers… does not automatically relaunch apps that were force quit"), [Downloading files in the background](https://developer.apple.com/documentation/foundation/downloading-files-in-the-background) (recreate the session when the system relaunches a terminated app), and [`application(_:handleEventsForBackgroundURLSession:completionHandler:)`](https://developer.apple.com/documentation/uikit/uiapplicationdelegate/application(_:handleeventsforbackgroundurlsession:completionhandler:)) (the reconnection hook, which `ForgeTransfers.handleEvents` implements).
- **First step:** decide where the songs live. Not Caches (iOS may purge them, losing offline music); a folder under Application Support excluded from backup. Confirm that the `transfers` service's own ledger (Application Support/ForgeTransfers) and inbox stay small enough not to matter.
- **Scope:** inside Soundscape PR 3, downloads go through the `transfers` service (its first real user). Framework PR: one kept-but-excluded class on `FileStore`, applied by the iOS adapter as `isExcludedFromBackup` on the folder, reapplied after the folder is created or replaced, since copies, replaces and atomic renames can drop the flag. File protection stays at iOS's default class unless an app asks for another.
- **What this proves, and what it doesn't:**

    | Capability | What Soundscape proves |
    | --- | --- |
    | Background transfers | Real application use, progress and completion |
    | Interrupted downloads | Continuation while suspended, reconnection after iOS ends the app, and app-managed rescheduling after a force quit |
    | Durable operations | Only that the `transfers` ledger survives termination and drives a transfer-specific recovery, not a general operation queue |
    | Generic retry policy | Nothing yet: it waits for a second app with genuinely similar retry needs |

- **Evidence apps:** Soundscape (first). A second user of the excluded class comes only when another app keeps re-downloadable data.
- **Dependencies:** Soundscape PRs 1 and 2 merged (1 is merged as #264).
- **Acceptance:** requirements for download, recovery after relaunch, airplane mode, and "not in backup"; Rust tests on the ledger; parity rule for caches without a class. The force-quit path is an explicit acceptance test, because this audit has not seen it end to end on an iPhone: the ledger code and `retry` exist, and Device Lab's 2026-11-11 sitting saw a force-quit row marked Interrupted (only on the second launch, since fixed in code) but never saw retry start a new transfer. Phase 3 is not done until a person records both on the phone.
- **Device:** three scenarios, recorded separately:

    | Scenario | Expected behaviour |
    | --- | --- |
    | App suspended (Home, screen locked) | The existing background transfer continues and completes |
    | App ended by iOS (hard to force; record if seen, else mark untested) | iOS relaunches the app in the background, the session is reconnected and the download is delivered |
    | Person force-quits mid-download | The task is cancelled; on the next manual launch the ledger reports the download as `Failure::Interrupted`, `retry` creates a new background task (a new task identifier in the log), and it completes |

    Then: airplane mode plays the downloaded songs; Settings › iCloud › Manage Storage shows the app's backup size without them.
- **Excludes:** a generic sync engine, a durable operation queue, `RetryPolicy` (waits for a second app with similar retry needs), CloudKit, explicit protection classes nobody asked for.

### Phase 4: One native-services route

This changes Forge's distribution model, not only where generated files go, so it starts with an investigation and becomes an implementation commitment only when that answers the questions below.

- **Investigate first:** whether optional UniFFI services can sit behind Cargo features of `forge-native` without bloating the core (binary size and the Swift a disabled service still needs); how an app selects services (Cargo features plus imports, and how that stays in step with `forge.toml` permissions, review P1); whether ForgeNative can ship each adapter so an outside app needs no ixOS checkout (SwiftPM has no per-feature compilation, so an unused adapter must still compile and link without its Rust symbols); how the copying route retires app by app without breaking any app mid-way; and whether the result is a clean release boundary for ForgeKit. Output: a short design note and a yes or no.
- **Objective:** an app adds a service by enabling a Cargo feature of `forge-native` and importing ForgeNative; nothing is copied into `Generated/`.
- **Scope:** optional services become features of `forge-native` with adapters in ForgeNative; a parity rule `forge-native`; apps move in waves (Soundscape and Weather, whose needs are small, first); the copying in `generate-bindings.sh` is removed when the last app moves. Platform limits (`MIN_REPEAT_SECONDS`) move to adapters in the same PR as notifications.
- **Evidence apps:** all; A35 is the motivating case.
- **Dependencies:** the investigation above; a ForgeKit release (0.7) and `forge-release.toml` update; Phase 3 should land first so `transfers` moves once.
- **Acceptance:** a fresh `forge init` app outside the checkout uses `http` and `location` with only the ForgeKit release; `check-ios-xtool.sh affected` green for moved apps.
- **Device:** Device Lab build with the moved apps; Soundscape plays and Weather finds location.
- **Excludes:** splitting ForgeNative into many packages; Android adapters.

### Phase 5: Now Playing helper

- **Scope:** first compare the two: Soundscape's queue (next, previous, seek, artwork) against Pocket Library's read-aloud (play, pause, skip by section). Extract only the interface they genuinely share; if it is just the info dictionary and play/pause, the helper is that small. Then a `.music` use in `ForgeAudioSession`; Soundscape and Pocket Library move onto it.
- **Acceptance and device:** lock-screen controls work for a song and for read-aloud in one sitting.
- **Excludes:** a Forge audio player; playback stays AVPlayer and AVSpeechSynthesizer.

### Already planned, not repeated here

Wave 5 (search into forge-storage after Pocket Library, chart shaping after Good Dog, photo picker after FieldKit photo notes) stays as written in roadmap.md; this plan only adds that the photo picker extraction is "picked item to staged `FileStore` file", not a picker view.

## Part 8: Deferred and rejected ideas

| Idea | Decision | Why | Revisit when |
| --- | --- | --- | --- |
| Generic repository layer, ORM or sync engine | Rejected | `Document` and `Database` cover every app; conflict rules are domain logic (review N5) | Never as a framework layer |
| A shared operation/outbox framework now | Deferred | One durable user (Chat); Soundscape downloads use `transfers`, which is a different shape | A second durable queue merges and passes on the phone |
| `RetryPolicy` in forge-net | Deferred | Needs two real retry sites to name what is common | Phase 3 plus Chat outbox compared |
| Universal settings screen or settings framework | Rejected | Apps organise settings differently; the Rust-document vs `@AppStorage` split already works | n/a |
| State restoration framework | Rejected | `@SceneStorage` and `NavigationPath` are Apple's answer | An app loses meaningful position on relaunch |
| Framework clock or time abstraction | Rejected | Passing `now` into Rust functions is enough and already used (Good Dog, Pocket Library) | A third app needs local-day logic, then a small `forge_runtime::time` helper |
| Rust permission abstraction for every Apple permission | Rejected | Native first: only permissions the Rust core reads get a Rust interface | n/a |
| In-app diagnostic export, MetricKit | Deferred | No app is in users' hands yet | First App Store release |
| Localising the apps | Deferred | No release target language; the tooling exists | The App Store app is chosen |
| Shared lifecycle phase hook (`on_transition`) | Deferred | One user (Chat, A31) | Relay's live connections need it |
| Background executor with priorities and pressure budgets (review R3, E1) | Deferred | No measured problem on the phone; thread counts are small | A jetsam or thermal problem is recorded on a device |
| Wrappers for Quick Look, haptics, share sheet, charts, maps | Rejected | One-line SwiftUI APIs; a wrapper adds nothing | n/a |
| Share extension, widgets, Live Activities, App Intents | Deferred | Extension packaging on xtool is unproven; roadmap names first and second users | A first user's feature is scheduled |
| Universal links, Sign in with Apple, purchases, push | Deferred | Paid Apple team required | Reuben pays for the team |
| Moving Relay's Noise channel into forge-net | Rejected | iroh replaces the wire; the channel's only other user is the device tools | n/a |
| New crates or Swift packages for any of the above | Rejected | Every recommendation fits an existing crate or product | A boundary becomes hard to maintain |

## Part 9: Immediate next steps

Don't start all six phases. Merge this audit as the research baseline, then do Phase 0 before any implementation, so the next wave starts from documentation that matches the code. They follow the merge of the open PRs (#268 to #271), keeping one thing at a time.

1. **Docs truth pass (Phase 0).** Fix the drift rows in Part 2 and add the foundation-review status update. Docs only.
2. **Phase 1's investigation: how a typed open failure reaches `ForgeRoot`.** A short design note (no code) answering the four questions in Phase 1, then the recovery screen proven in Sketchbook.
3. **Where Soundscape's offline songs live (Phase 3's first step),** decided inside Soundscape PR 3 when it starts, with the excluded class arriving with its first user.

Then Phase 2 (permissions) with FieldKit's next feature, Phase 4 (one native-services route) when no app PR is mid-flight, and Phase 5 (Now Playing) whenever Soundscape or Pocket Library next touches playback.

**Decisions for Reuben** (none blocks step 1): whether Phase 1 may set the damaged file aside automatically or should ask first; whether Phase 4's ForgeKit 0.7 should wait for Phase 1 so outside apps get both in one release.

## Part 10: Documentation update proposals

No repository files were changed. These edits are proposed for Phase 0, with `docs/areas.md` as the authority.

| Document | Change | Why |
| --- | --- | --- |
| `docs/areas.md`, `docs/boundaries.toml` | Relay → forge-devproto crossing: plan becomes "removed when Relay's iroh transfer replaces the Noise channel" | Roadmap 2026-11-16 excludes moving it to forge-net |
| `docs/architecture.md` | Crates table: add `forge-app`, `forge-native`, `forge-reader`; drop `chat-core` (app); remove "move to it in wave 3"; layers table: Forge Device is built | Out of date since wave 3 and #246 |
| `docs/architecture.md` (new short section) | "Everyday capabilities: where they live", a 10-row table pointing to the owner (Forge, Apple, app) from Part 5 | Answers "does Forge do X?" in one place |
| `docs/roadmap.md` | Remove "installed over the air"; M3 "Next" list updated to what is built; add Phases 1 to 5 as a short section under Waves 4 and 5, each with its proving feature | OTA parked; avoid a second roadmap |
| `docs/foundation-review.md` | Status line: R1 fixed (forge-app), R4 partly fixed (finishing, launch report), N4 built, S5 and P2 open and scheduled (Phases 3 and 2) | The review is cited as current |
| `docs/native-services.md` | Say which route is current (forge-native) and that the macro route is being retired; list optional services as future Cargo features | Two routes confuse a newcomer |
| `docs/native-capability-roadmap.md` | Mark built items; keep the `notify.rs` limit as an open departure scheduled with Phase 4 | Drift |
| `docs/persistence.md` | Add "opening data that can't be read" (after Phase 1) and "storage classes and backups" (after Phase 3) | Completes the data lifecycle story |
| `docs/ios-ui.md` | Patterns, no new API: `@SceneStorage` for position, settings split (domain in Rust, display in `@AppStorage`), the permission denied state (after Phase 2) | Apple-first guidance instead of wrappers |
| `docs/app-crate.md` | "Pass `now` in" as the rule for time in Rust; user-facing words belong in Swift, Rust returns kinds | Testability and localisation |
| `docs/localization.md` | Note that no app ships a catalog yet and that ForgeKit's own strings are not localisable | Honest status |
| `docs/app-parity.md` (via toml and the script) | New rules as phases land: `open-recovery`, `permission-denied`, `forge-native`, `no-print` | Rule 12: no app left behind |
| Ledger records | No status changes from this review: nothing new was run on a phone | Rule 7 |
