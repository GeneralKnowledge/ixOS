# Architecture assessment

Reviewed at `420969d` (main, 2026-11-10), against the application model that is now canonical: `forge-app` (Rust runtime) plus ForgeKit (Swift package), as merged in #141, #142 and #143, with ForgeAppKit and the `document` and `validation` foundations from #146. The older Counter design is not the target, and is assessed only where documents still describe it as the target.

[architecture.md](architecture.md) explains how the pieces fit. This page judges them: what the design is deliberately trading away, what is only a limitation of the current code, and where it should improve. It turns Reuben's review of 2026-11-10 into a checked list: each recommendation is marked **done**, **partial**, **missing** or **deferred** (deliberately, with the reason), with the files involved and the change that would close it.

It builds on the [foundation review](foundation-review.md) (primitives, finding ids such as S4 and N1) and the [app model review](app-model-review.md) (phases 0 to 5), and does not repeat them.

## Verdict

Keep the foundation: Rust owns state and rules, SwiftUI presents them natively, UniFFI joins them with pull-based events and async commands, and the `forge` CLI builds everything from Linux. None of the findings below needs that changed, and several proposed improvements would damage it if done carelessly (see [Intentional design](#intentional-design-keep-it)).

The gaps are in what the runtime *guarantees*, not in how apps are structured:

1. **The canonical model exists but is not yet the only one.** Both starters and Checklist are on `forge-app` + ForgeKit. Chat, Good Dog, Weather, Counter and Device Lab are not, and Pocket Library uses only `forge-app`'s validation. The README and `architecture.md` still teach Counter's design.
2. **Persistence is safe per file, not per application.** The pieces (atomic writes, SQLite transactions, versioned documents, set-aside recovery, orphan sweeps) are good. What is missing is the layer that turns them into a promise: a commit protocol for a record plus its files, storage classes for backup and protection, and fixtures that prove version N's data opens in N+1.
3. **Two of the four kinds of work are missing.** Committed and foreground work are implemented and tested. Finishing work (`beginBackgroundTask`) and durable work (survives termination) are not, apart from Chat's outbox, which is app code.

Everything else (Swift ergonomics, invalidation areas, networking policy, a verification suite) is worth doing, but after those three, in the order at the end of this page.

## Intentional design: keep it

These are choices, each with a real cost. A recommendation that removes one should be read as a change of product direction, not as an improvement.

| Choice | What it buys | What it costs | Don't "fix" it by |
|---|---|---|---|
| **Rust owns state and rules; Swift presents** | One tested implementation of the domain, runnable on Linux; Swift stays presentation | Every new screen needs an FFI query or command; a Swift developer must touch Rust to change a rule | Moving validation or decisions into Swift models for convenience |
| **Native SwiftUI, no Forge UI layer** | Apps look and behave like iOS apps; Apple's tools, accessibility and previews work | No shared UI code with Android; Forge cannot generate screens | Adding a renderer, a DSL, a navigation framework or Rust-driven views |
| **Revision invalidation, not state snapshots** | `Changed { revision }` is cheap over FFI, never inconsistent, and needs no diffing; models re-query what they show | A model may re-read more than changed; very large lists need queries that page | Sending whole state over FFI, or building a reactive store or object graph |
| **Pull-based, bounded event streams** | Backpressure, no Swift code on Rust threads, no observer leaks; drops are explicit (`EventsDropped`) | Each consumer runs a loop; a slow consumer sees drops and must re-read | Switching to UniFFI callback interfaces for events |
| **One serial executor per core** | Commands never interleave; no lock is held across a save; commit-then-publish ordering is guaranteed | One slow command delays the next; long work must be a separate operation | Making commands concurrent or cancellable once accepted |
| **The runtime does not own the app's state type or storage** | Apps choose JSON, SQLite, files or nothing; `forge-app` stays small | Each app wires its own store into the executor; fewer things can be automated | Introducing a framework-shaped model, ORM or repository layer |
| **No generic job system until a second user** | Chat's outbox can become a contract shaped by real use, not guesses | A second offline app today would copy Chat's code | Building a scheduler or sync engine ahead of FieldKit or Relay |
| **Native first** | Apple's frameworks (URLSession, Keychain, BackgroundTasks) do what they already do well | Two implementations behind some traits (`UreqClient` on Linux, URLSession on iOS) | Reimplementing platform behaviour in portable Rust |
| **ForgeUI standalone; ForgeAppKit optional** | The design system is useful in any SwiftUI app; adoption is incremental | Integration between the design system and the runtime lives in app code | Making ForgeUI depend on ForgeKit or Rust |
| **Linux first, human first** | Everything builds, tests and installs from Linux with the CLI; agents are optional | Swift UI code compiles only with the Darwin SDK or on macOS CI | Adding steps that need Xcode, a Mac or an agent |
| **Extract on the second use** | The framework holds proven code only | Duplication between apps until the second user arrives | Lifting code from one app into the framework speculatively |

## Current limitations: fix them

These are not choices. They are where the code has not caught up with the design it has already chosen.

| Limitation | Where | Why it is not intentional |
|---|---|---|
| Five apps are not on the runtime | `examples/{chat,good-dog,weather,counter,device-lab}` | The app model review chose one architecture (phase 5 migrates the apps) |
| The README and `architecture.md` describe Counter as the reference | See [Counter-era descriptions](#counter-era-descriptions-to-reconcile) | Superseded by #141 |
| No finishing scope (`beginBackgroundTask`) | `forge-app` has `foreground` and `lifetime` only (`crates/forge-app/src/runtime.rs`); ForgeKit reports phases but never asks for time (`forgekit/Sources/ForgeKit/ForgeLifecycle.swift`) | App model phase 2 lists it; foundation review R4 |
| No durable work outside Chat | `examples/chat-core/src/client.rs` (outbox) | Deferred to a second user on purpose (N1), but the contract is undefined, so nothing else can be durable yet |
| A record and its files are committed by convention | `crates/forge-storage/src/files.rs` (`place` renames without syncing the directory; `sweep` is app-called) | Foundation review S4 |
| No backup or protection classes per store | `ForgeEnvironment` has data and caches directories with no policy; `forge.toml` sets one app-wide `data_protection` | Foundation review S5; `ForgeHost.swift` says "phase 4 adds the policies" |
| No historical data fixtures | No app crate tests open an old file | Foundation review S7 |
| Framework Swift still copied into `Generated/` | `scripts/generate-bindings.sh` copies `ForgeServices*.swift`, `ForgeLog.swift`, `ForgeImages.swift` | App model phase 2 moves them into ForgeKit |
| Each app writes its own error text table | `templates/ios-app/Sources/AppHost/NotesModel.swift` (`describe`, plus a separate `isRetryable` switch) although `ForgeErrors.describe(kind)` exists | F4 is solved in Rust (`ErrorKind`) but not used fully in Swift |
| Releases lag the checkout | `forge-release.toml` says 0.3.0; `forge-app` is 0.4.0, so `forge init` outside the checkout points at the checkout by absolute path | Phase 3 (independent apps) is built, but only works while a release is current |
| Retry is only `Backoff` | `crates/forge-net/src/backoff.rs`; no classification of `NetError`, no `Retry-After`, no attempt limit | Foundation review N2 |

## Recommendations, checked against the code

Summary first; each row has a section below.

| # | Recommendation | Status | Main gap |
|---|---|---|---|
| 1 | Canonical architecture is the only iOS starter | **Partial** | Starters done; five apps and the main docs are not |
| 2 | Persistence as an application guarantee | **Partial** | Record-plus-file commits, storage classes, fixtures, upgrade tests |
| 3 | Complete lifecycle and durable work | **Partial** | Finishing scope missing; durable work only in Chat |
| 4 | Swift developer experience | **Partial** | Error text, service adapters, entry points, multiple scenes |
| 5 | State-change contract with areas | **Missing** (low cost) | `Changed { revision }` has no areas |
| 6 | Reusable networking policies | **Partial** | Retry classification, offline recovery, URLSession, queues |
| 7 | Application verification suite | **Partial** | Kill/relaunch on device, upgrade, offline, background |
| 8 | Simpler overall developer experience | **Partial** | Two places to work exist; the crate graph still leaks |

### 1. Canonical architecture

**Partial.**

Done:

- `templates/app-rust` and `templates/ios-app` are on `forge-app` + ForgeKit; `templates/ios-minimal` is the same architecture trimmed (one `Package.swift` with ForgeApp and ForgeKit only, pinned by a test in `crates/forge-cli/src/commands.rs`). The `--minimal` split that the app model review called out is closed.
- `examples/checklist` is the reference app on the runtime, with Rust tests (`rust/tests/core.rs`) and Swift tests that drive the real core (`Tests/ChecklistTests`), on Linux and macOS (`.github/workflows/app-model.yml`).
- ForgeKit is versioned and released into its own repository; `forge init` outside the checkout pins it from `forge-release.toml`.

Not done:

- **Apps.** Pocket Library uses `forge_app::Validator` and `FieldError` but keeps its own `Mutex<State>`, `background()` calls, `ensure_running`, `parts()` and its own `Changed { revision }` event (`examples/pocket-library/rust/src/lib.rs`). Chat, Good Dog, Weather, Counter and Device Lab use none of `forge-app`.
- **Docs.** The README and `architecture.md` present Counter's `CounterBridge` as the pattern ([list below](#counter-era-descriptions-to-reconcile)).
- **Release currency.** `forge-release.toml` is at 0.3.0 while `forge-app` is 0.4.0, so a new app made outside the checkout uses absolute paths until 0.4.0 is released and its tag pushed.

Changes, in order:

1. Rewrite `architecture.md` around `forge-app` + ForgeKit, with Checklist as the reference; move Counter's command-thread and persistence sections to a short history note or into `examples/counter/README.md`. Update the README's opening and command examples to Checklist. (Docs only; the next PR after this one.)
2. Release ForgeKit 0.4.0 and update `forge-release.toml`, so new apps are independent again.
3. Move Pocket Library onto `Runtime` before its App Store submission (app model phase 5; it fixes the data layout before users have data). Then Good Dog, then Chat (whose outbox becomes the durable contract, section 3).
4. Decide Counter's fate: keep it as the Android twin's crate, but stop calling it the reference app anywhere.

### 2. Persistence guarantees

**Partial.** The primitives are strong; the application-level promise is not yet written down or enforced.

| Guarantee | Status | Where |
|---|---|---|
| Atomic writes, crash-tested with SIGKILL | Done | `forge-storage` `write_atomic`, `JsonFile`, `Database`; `crates/forge-storage/tests/foundation.rs` |
| Schema versions and migration hooks | Done (mechanism) | `JsonFile::with_migration`, `Database::open(path, migrations)`, `forge_app::Document::open_with_migration` |
| Newer data refused, never overwritten | Done | `DocumentError::Newer`, `Database` refuses a newer schema |
| Unreadable data set aside, only after the person agrees | Done for documents | `forge_app::document::set_aside`, `Document::start_fresh`; Pocket Library's `open_fresh` |
| One writer per data directory | Done | `DataDirectory` lock (`crates/forge-app/src/runtime.rs`), tested in Rust and Swift |
| Automatic migration conventions | Partial | Hooks exist; the old file is kept before the first write in a new schema ([persistence.md](persistence.md)); no SQL plus Rust data steps yet (S7) |
| Tests with real historical data | Done (convention) | `fixtures/` per app crate with `every_saved_version_still_opens`: Pocket Library, Checklist |
| Backup and file-protection policy | **Missing** | No per-store classes; `ForgeEnvironment` has no policy; one app-wide `data_protection` in `forge.toml` (S5) |
| Recovery from corruption | Partial | Documents and SQLite have it, and moved-aside copies are capped at three; no shared recovery screen |
| Record plus external file, committed together | Done | `FileStore::stage_write` / `Staged::commit`, directory sync; Pocket Library's imports use it |
| Upgrade testing between released versions | **Missing** | Nothing has been released yet; the device check is planned per release |

**Worked example: Pocket Library imports an EPUB** (`import_epub`, `examples/pocket-library/rust/src/lib.rs`).

The sequence today is: save the cover file, save the EPUB file (deleting the cover if that fails), then add the book and save `library.json`. At every launch, `Store::remove_orphan_covers` (`store.rs`) sweeps `covers/` and `books/` for files no record names. Deleting a book does the reverse: save the record without the book, then delete its files.

So the protocol is already the right one, *files first, record second, sweep orphans at launch*:

| Crash point | Result |
|---|---|
| After the cover, before the EPUB | Orphan cover; swept on next launch. No book, as the person saw no success |
| After both files, before `library.json` | Orphan files; swept. No book |
| After `library.json` | Book and files present |
| During a file write | Hidden temporary; swept |

What is missing is the guarantee around it:

- **Power loss.** `FileStore::place` syncs the file but not the directory after its rename, unlike `write_atomic`. After a power cut a committed record can name a file whose rename was lost (S4). Fix in `crates/forge-storage/src/files.rs`.
- **It is a convention.** Each app has to know the order and remember the sweep. Chat keeps attachments in its own `FileStore` with its own bookkeeping. That is a second user, so S4's proposal is due: a staged-file API (`stage` returns a token, a command commits the token together with its record, the runtime sweeps unclaimed staged files at launch). It belongs in `forge-storage` and is called from inside a `forge-app` command, so "committed before success is reported" covers the files too.
- **No test of the crash points.** `forge-storage`'s foundation tests cover the protocol in general; Pocket Library has a unit test of the sweep but none that kills the process between the steps. Add one per app that uses files, using the foundation tests' re-exec helper.

Changes, in order: directory sync in `FileStore` (small, now); the staged-file API, with Pocket Library and Chat moved onto it; storage classes in `ForgeEnvironment` (`data`, `caches`, a re-downloadable media class excluded from backup, file protection per class) applied by ForgeKit at open; a `fixtures/` directory per app crate with a test that every released schema still opens; a copy of the file before any migration. All of it is app model phase 4, now with Pocket Library as its first user.

**Status (2026-11-11).** Done: directory sync, the staged-file API with Pocket Library on it, a copy before migrating, capped recovery copies, and fixtures for Pocket Library and Checklist ([persistence.md](persistence.md)). Chat's attachments are content-named, so they are immutable and need no staging. Its records and files are already consistent under the hashed store. The crash points are now the API's, so the foundation kill test covers them with staging, and a per-app kill test would only repeat it; each app's own part is the sweep predicate, which its unit tests cover. Still open: storage classes (S5, next), and a Rust data-migration hook for SQLite.

### 3. Lifecycle and durable work

**Partial.** Reuben's four kinds of work against the code:

| Kind | Expected | Status | Where |
|---|---|---|---|
| Committed | Saved before success is reported | **Done**, tested | `Runtime::command` publishes `Changed` only after the closure returns `Ok` (`crates/forge-app/src/runtime.rs`); `forge-app/tests/runtime.rs`, Checklist's restart test |
| Finishing | Best-effort time to finish when backgrounded | **Missing** | No `beginBackgroundTask` in ForgeKit; no `finishing` scope in `forge-app` (R4) |
| Foreground | Stops when the app leaves the foreground | **Done**, tested | `Runtime::foreground`; `Interrupted` kind; `PhaseChanged { resumed }`; Checklist Swift test `testTheBackgroundInterruptsOnScreenWorkOnly` |
| Durable | Survives termination, resumes after relaunch | **Partial**: Chat only; **deferred** as a framework contract | Chat's outbox (`examples/chat-core/src/client.rs`) |

Also present: `lifetime` work that freezes with the process and stops at shutdown, application-level phases from `UIApplication` notifications (not a scene's phase), and a non-blocking `shutdown()`.

**Worked example: Chat's outbox.** A message that cannot be sent stays `Sending` in Chat's SQLite store. One outbox thread sends queued messages oldest first, with `forge_net::Backoff` between attempts, whenever a stream reconnects, the app resumes or a refresh succeeds, and at launch it sends what a previous run left behind. Each message carries a client-chosen id, so the server ignores a repeat and a send whose reply was lost is never duplicated. Attachments are uploaded first and remembered, so a retry does not upload them twice.

That is the reference implementation Reuben describes, and the right thing to extract *as a contract*, not as a job system: a persisted operation record (id as idempotency key, attempts, next attempt time, terminal failure), a retry classification, a replay hook at launch, and a reconciliation callback the app writes (N1). What should stay in Chat: what a message is, when to give up, how a failure is shown.

What Chat's outbox does not have, and the contract should add: a link to the runtime's lifecycle (it runs on its own thread, and Chat's own code wakes it on resume), an attempt limit or deadline while the server is unreachable, and the finishing scope, so the last send before backgrounding gets the extra seconds iOS grants.

Changes, in order:

1. **Finishing scope now.** `Runtime::finishing(host)` in `forge-app`, cancelled on expiry; ForgeKit wraps it in `UIApplication.beginBackgroundTask` from `ForgeLifecycle` on `didEnterBackground`, ending the task when finishing work drains. Linux test: expiry cancels it. One short phone check: lock the phone during a finishing job and read the log.
2. **Relaunch report.** `Opened { fresh, restored, migrated, recovered }` from the runtime at open, so apps can tell the person once that something was interrupted. Cheap and independent of durable work.
3. **Durable contract when the second user arrives** (FieldKit's upload queue, Relay, or Pocket Library's background downloads), with Chat migrated onto it in the same PR. Until then, keep it deferred and say so in `app-model-review.md`, as it already does.
4. `BGTaskScheduler` and background `URLSession` stay in the [native capability roadmap](native-capability-roadmap.md) (Tier 1, items 5 and 6). They add opportunistic progress to durable work; they do not replace it.

### 4. Swift developer experience

**Partial.**

| Item | Status | Where |
|---|---|---|
| Observable application model | Done | `ForgeHost` is `@Observable`; models re-query when `host.changes` moves (`forgekit/Sources/ForgeKit/ForgeHost.swift`) |
| Clear ownership of the core | Done | `ForgeHost` owned by the `App`, opened once off the main actor; `ForgeRoot` shows loading, ready and failed |
| Cancellation that works with Swift concurrency | Partial | `withRustCancellation` forwards Task cancellation, but each call site has to use it (F2) |
| Consistent error presentation | Partial | `ErrorKind`, `ForgeKindedError`, `ForgeErrors.describe(kind)` and ForgeAppKit's `ForgeFailure` exist; the template still writes a full `describe` table and its own retryable switch |
| Native navigation and presentation | Done, deliberately small | ForgeAppKit destinations and alerts as model values; SwiftUI's own navigation |
| Native lifecycle integration | Partial | `ForgeLifecycle` reports application phases; no background task, no protected-data or memory-pressure phases (E1) |
| Multiple windows | **Deferred** | One core per process is right; a second scene must share the host, not open its own. Untested |
| App extensions | **Deferred** | A `shared/` store design is in the app model review; waits for the first extension |
| Framework Swift is a dependency, not copied files | Partial | ForgeKit, ForgeUI, ForgeAppKit are packages; service adapters, `ForgeLog` and `ForgeImages` are still copied into `Generated/` |
| One entry point | **Missing** | Three `@main`s per app (xtool library product, `swift run` launcher, Xcode), each repeating the scene |
| ForgeKit's own tests | **Missing** | `forgekit/Tests` has ForgeUI and ForgeAppKit tests only; `ForgeHost` is tested only through Checklist |

Changes: make `ForgeErrors.describe` the default in the template so an app writes copy only for its domain cases, and derive "retryable" from `ErrorKind`; generate `withRustCancellation` wrappers for exports that take a `Cancellation` (F2), or at least lint for calls that skip it; move the service adapters into a ForgeKit product selected by `forge.toml`; add a `ForgeKitTests` target that drives `ForgeHost` against a small fake core; reduce the `@main`s to one scene definition shared by the three entry points. ForgeKit should keep making the common path short without wrapping SwiftUI: no ForgeKit navigation stack, no view base classes.

### 5. State-change contract

**Missing, cheap, worth doing with care.**

Today: commands commit, the revision rises, `RuntimeEvent::Changed { revision }` is published (`crates/forge-app/src/contract.rs`), and every model re-queries when `host.changes` moves. `EventsDropped` and resume trigger a reconcile.

Reuben's proposal, `Changed { revision, areas }`, fits the design: it adds information without adding machinery. Two cautions keep it simple:

- Areas are app-declared names (`"books"`, `"progress"`), returned by the command closure, and an empty list means "anything may have changed". A model that ignores areas stays correct.
- `EventsDropped`, resume and a missing area must still mean "re-read everything". Areas are a hint for cost, never the only route to correctness.

Change: an `areas: Vec<String>` field on `Changed` (a contract version bump in `forge-app` and ForgeKit), `Runtime::command` gains a variant whose closure returns its areas, and `ForgeHost` exposes `changes(in: area)` alongside `changes`. Do it when an app shows a measured cost from re-reading (Pocket Library's statistics screen is the likely first), not before.

### 6. Networking

**Partial.**

| Item | Status | Where |
|---|---|---|
| HTTP client abstraction, HTTPS, cancellation, streaming | Done | `forge-net` `HttpClient`, `UreqClient`, `CancelHandle`, `open_stream_cancellable` |
| Request timeouts | Done | `Request::timeout` |
| Consistent error classification | Partial | `NetError` (`Offline`, `Timeout`, `Certificate`, `Transport`, ...) is shared; no "worth retrying" classification on it |
| Retry and backoff | Partial | `Backoff` with jitter; no attempt limit, deadline or `Retry-After` (N2) |
| Offline detection and recovery | Partial | The optional `connectivity` service reports state; nothing triggers a retry when the network returns. Chat flushes its outbox on reconnect by its own code |
| Request deduplication | **Missing** (framework); Chat-only | Client-chosen ids in Chat |
| Persistent outbound queue | **Deferred** | The durable contract in section 3 |
| Upload and download progress | Partial | `download_to_file` streams to a file; no progress reporting; no background transfers (N4) |
| URLSession transport on iOS | **Decided, not built** | Native capability roadmap, 2026-11-10 |

The split Reuben describes is the right one and matches the native-first rule: the framework provides the machinery (classification, a `RetryPolicy` beside `Backoff`, a connectivity-returned signal into the runtime, progress), and the app decides policy (a chat message retries for a day, a weather refresh once, an upload until the person cancels). Changes: `NetError::is_retryable()` and a `RetryPolicy` in `crates/forge-net`; feed `connectivity` changes into the runtime as a resume-like signal; progress on `download_to_file`; the URLSession transport, starting with background transfers, as already planned.

### 7. Verification

**Partial.** This is the strongest area, and the table shows where it stops.

| Test | What it proves | Status | Where |
|---|---|---|---|
| Kill and relaunch | State survives process termination | Partial | Storage SIGKILL tests (`forge-storage/tests/foundation.rs`); Checklist restart is in-process; Checklist after a kill on the iPhone passed once (2026-11-10, reported by Reuben). No app-level SIGKILL test |
| Upgrade N to N+1 | Old data migrates | **Missing** | No fixtures; no release yet |
| Corrupt storage | Recovery is safe and explicit | Partial | `forge-app` document tests, `Database` recovery tests, Pocket Library tests; not run for every app |
| Slow network | UI stays responsive | Partial | Queries never wait for commands (`queries_never_wait_for_a_slow_command`); no slow-network harness for apps |
| Offline then online | Requests recover | Partial | Chat client tests with an in-memory repo; nothing generic |
| Background and resume | Scopes behave | Done on Linux and in Swift | `forge-app/tests/runtime.rs`, Checklist Swift tests; no device record yet |
| Multiple core instances | Storage ownership enforced | Done | `a_data_directory_has_one_holder`, `testOneCorePerDataDirectory` |
| FFI compatibility | Bindings stay aligned | Done | `scripts/check-bindings.sh`, contract version check in `ForgeHost.start()`, the published-ForgeKit CI job |

Change: a `forge_app::testing` harness (open a core on a directory, kill a child process mid-command and reopen, load a fixture, swap in a slow or offline `StaticClient`), so each reference app runs a common subset with a few lines of test code. Start with kill-and-relaunch and fixtures, since those protect user data. The device-only parts (suspension timing, background-task expiry, a real upgrade) become one Device Lab check each, run per release, not per change.

### 8. A simpler overall developer experience

**Partial.**

`forge init MyApp` then `forge run ios` works from Linux, and a developer mostly works in `Sources/` and `rust/`. What still leaks:

- `rust/Cargo.toml` lists `forge-app`, `forge-runtime`, `forge-storage`, `forge-net` and `uniffi` (pinned exactly) by path or git rev, and the starter's Rust imports from all four Forge crates. A `forge` facade crate re-exporting what an app needs would make the crate graph an internal detail.
- `forge.toml`, `Cargo.toml` and `Package.swift` each carry the framework version; nothing checks they agree, and `forge.toml` has no `[forge] version` yet (app model phase 3).
- `Generated/` is committed and must be regenerated by hand after an API change (`forge bindings generate`); `forge build` could regenerate and fail only when the committed copy differs.
- The library must be named `forge_ffi`, so the app's own name is not its crate's name.

The layering Reuben proposes (SwiftUI, the app's Rust crate, Forge, with UniFFI and ForgeKit as the joints) is the one the code now has; these changes make it visible in fewer files. They come after persistence and lifecycle, because they change no guarantee.

## Counter-era descriptions to reconcile

Places that describe the pre-#141 design as current. Fixing them is the first change under recommendation 1.

| File | What it says | Now |
|---|---|---|
| `README.md` (intro) | "ships a working **Counter** example"; "SwiftUI talks to a thin Swift layer (`CounterBridge`)" | Checklist and the starters on ForgeKit; no per-app bridge |
| `README.md` (commands) | Every `forge build/run/diagnose` example uses `examples/counter`; "see `examples/counter/forge.toml`" | Use Checklist or a fresh `forge init` app |
| `docs/architecture.md`, opening and *Layers* | Layers list `forge-runtime`, `forge-storage`, `forge-net`; no `forge-app` or ForgeKit; "What every app crate gets from `forge-runtime`" includes `app_shell!` | `forge-app` and ForgeKit are the runtime layer; `app_shell!` remains only in the apps not yet on the runtime (Chat, Good Dog, Pocket Library) and the Android starter |
| `docs/architecture.md`, *Counter, the reference app* | Counter's `CounterBridge` layer table; "Add a bridge layer like Counter's" | `ForgeHost` + `@Observable` models; Checklist is the reference |
| `docs/architecture.md`, *Ownership*, *Persistence*, *Concurrency*, *Startup*, *Schema versions* | Counter's `JsonSnapshotStore`, `forge-counter-v1.json`, storage directory chosen in Swift, `recover_counter_app` | `forge_app::Document`, `DataDirectory` lock, `ForgeEnvironment`, `start_fresh` |
| `docs/architecture.md`, *Crates* | No `forge-app` row | Add it |
| `docs/ffi-policy.md`, *Binding drift* step 3, *Interaction model*, *Lifecycle and ownership*, *Error categories*, *Exposing a new operation*, *Consuming it from SwiftUI*, *Testing* | Counter's API, `CounterClient`, `CounterError`, `ObservableObject` view model; "Copy the bridge change to `templates/ios-app/Sources/CounterBridge/`" (that folder no longer exists) | Rewrite with Checklist and the template; `ErrorKind` for categories; `withRustCancellation` |
| `docs/ideal-design.md` | Describes `forge-core`, `forge-ffi`, `ForgeCounterApp` | A historical inferred-design document; mark it superseded and link here |
| `docs/roadmap.md` (first line) | "a working Counter app on iOS" | The application runtime and its reference apps |
| `docs/compatibility.md`, `docs/toolchain-validation.md` | Manual acceptance is "Counter on device" | Use Checklist or a fresh starter |

`docs/ios-ui.md` already says Counter, Weather and Chat are older code, and `docs/app-crate.md` already describes the runtime; they need no change.

## Order of work

Reuben's order, with the concrete first step for each. Each is small enough to review alone; testing scales to the change.

| Step | Theme | First PR | Size |
|---|---|---|---|
| 1 | Canonical architecture | Reconcile `architecture.md`, `ffi-policy.md` and the README with the runtime (docs only); release ForgeKit 0.4.0 and update `forge-release.toml` | 1 docs PR, 1 release |
| 2 | Persistence | `FileStore` directory sync; staged-file API used by Pocket Library's EPUB import and Chat's attachments; fixtures directory and test convention; storage classes in `ForgeEnvironment` | 2 to 3 PRs |
| 3 | Lifecycle and durable work | Finishing scope with `beginBackgroundTask`; relaunch report; Pocket Library onto `Runtime`. Durable contract waits for its second user | 2 PRs, one short phone check |
| 4 | Swift developer experience | Default error text by kind; ForgeKit tests; service adapters into ForgeKit; one entry point | 2 PRs, macOS CI |
| 5 | Networking | `is_retryable` and `RetryPolicy`; connectivity into the runtime; then the URLSession transport per the roadmap | 1 PR, then the roadmap item |
| 6 | Verification | `forge_app::testing` harness; common subset run by Checklist, Pocket Library and Chat | 1 to 2 PRs |

Invalidation areas (recommendation 5) are not in the order: add them when a screen shows a measured cost.

## The layers, as Reuben framed them

- The **architecture** defines how applications are structured: SwiftUI screens, the app's Rust crate, Forge underneath.
- The **runtime** (`forge-app`) makes that structure reliable: commits, lifecycle, scopes, the data directory.
- **ForgeKit** makes it natural to use from Swift: one host, observation, cancellation, errors.
- The **CLI** makes it practical to build and maintain, from Linux, with or without an agent.
- The **application** remains free to define its own domain and experience.

Every recommendation above moves work into one of the first four lines without taking anything from the fifth.
