# What is an ixOS application? (app model review)

Review of `629c501` (main, 2026-11-09), done for Reuben's question "do these pieces form a usable application framework, or a collection of Rust libraries, Swift adapters and build tooling that happen to produce an iOS app?". It has two parts: an [assessment of the current code](#part-1-assessment) and a [proposed canonical application model](#part-2-the-canonical-ixos-application) with a [phased plan](#part-3-phased-plan). Nothing in it is implemented.

It builds on the [foundation review](foundation-review.md) (PR #129, fixes in #133) and does not repeat it. That review audited the primitives (storage, runtime, services) and found them sound but unowned. This one looks one level up: the application those primitives are supposed to add up to, as `forge init` generates it and as the demos built it. Finding ids from the earlier review (R1, S5, F4, ...) are referenced, not restated.

Method: `forge init Full` and `forge init Mini --minimal` were generated outside the checkout and read file by file. Chat, Good Dog, Pocket Library and Weather (plus Counter, the documented reference app) were compared on the same questions: who owns the Rust instance, how commands, state and events flow, what happens across the lifecycle, how data is stored, what the app depends on. Everything below was checked in code; where a document says something else, that is called out.

**Verdict in one paragraph.** The division of labour is right and should stay: Rust for state and rules, SwiftUI for presentation, UniFFI as the boundary, Forge as the tooling. What is wrong is structural and is not fixed by more primitives: **there is no application in the framework.** The framework provides crates and three macros; *the template is the framework*, delivered by copying. Each app then hand-assembles its own core object, its own state model, its own lifecycle handling and its own Swift plumbing: the two starters and five apps show six different shapes. The Swift half of the framework is not a library at all: it is files copied into `Generated/` at build time plus byte-identical copies in every app. The Rust instance's lifetime is owned by a SwiftUI view rather than by the app. And a generated app is bound to a specific, unversioned checkout by absolute paths, so it is neither independent nor reproducible. None of this needs a rewrite, but it does need one new layer (an application runtime, in Rust and in Swift) and a change to how the framework is distributed. These should come before more demos are built on the current shape, and before Pocket Library ships.

---

## Part 1: assessment

### 1. What an ixOS application is today

There is no definition in code. An app is whatever `templates/app-rust` plus `templates/ios-app` produce, edited. Counting the shapes that exist:

| App | Rust core object | State reaches Swift by | Lifecycle | Concurrency | Swift plumbing |
|---|---|---|---|---|---|
| `forge init` (full) | `AppCore`: `Mutex<State>`, `EventHub`, `Lifecycle`, `shut_down` flag, `UreqClient` | Command returns the item; `NotesChanged { notes }` carries the whole list; model also re-reads after each command | `set_phase`, foreground scope | `background()` + mutex held across the save (R1) | Template copies |
| `forge init --minimal` | `AppCore`: `Mutex<Saved>`, `JsonFile` | Each command returns the whole list; no events | None | Same as full | None (no lifecycle, no services) |
| Good Dog | `AppCore`, same fields as full | `DataChanged` (invalidate, re-read) | `set_phase` | Same as full | Template copies |
| Pocket Library | `Library`: own `lifetime: CancelHandle`, no `Lifecycle` | `Changed { revision }` (invalidate, re-query) | None in Rust; Swift calls `becameActive()` | `background()` + mutex; own `cancellable()` helper | Own event loop; no `AppServices` |
| Chat | `ChatCore` + `chat-core` client thread | Fine-grained deltas (`MessageReceived`, `ConversationUpdated`, ...) | `set_phase`, closes streams in background | Own threads, SQLite, outbox thread | Template copies |
| Weather | `ForgeWeatherApp` with its own `signal.rs` | `WeatherOperation` objects; no `EventHub` | None | A thread per refresh operation | Older `WeatherBridge` layer |
| Counter (the reference in `architecture.md`) | `ForgeCounterApp` with a serial command thread | `StateChanged { snapshot }` | `Lifecycle` | Command thread (the design R1 recommends) | `CounterBridge` layer |

So the answer to "is there one clear, canonical application architecture?" is no. Good Dog has the full starter's shape; every other row differs. There are five state models (returned value, full snapshot event, invalidation, deltas, operation objects), four concurrency models (mutex across the save, a serial command thread, a thread per operation, Chat's own threads), and three lifecycle stances (reported, ignored, Swift-only). Each is defensible on its own; together they mean nothing about an ixOS app can be assumed by the framework, by tooling, or by an agent working on it.

The same plumbing is re-written in each core: a `shut_down: AtomicBool` with `ensure_running()` (template, Good Dog, Chat, Pocket Library), a `parts()` helper that clones `Arc`s into a closure (template, Pocket Library), `ShutDown`/`Cancelled`/`Interrupted` error cases mapped by hand, and cancellation wiring that links a host `Cancellation` to a scope (template `check_url`, Pocket Library `cancellable`). In the full template, the notes themselves (validation, add, update, delete, search) are a minority of the 460 non-test lines of `rust/src/lib.rs`; the rest is this plumbing, the URL check's wiring and the devtools hook.

### 2. The application core and framework capabilities are not separated

Reuben's model has two Rust layers: an application core (the app's state, domain and decisions) and framework capabilities (persistence, networking, device services, background work). In the code there is only one layer, the app's `#[uniffi::export] impl`, and it reaches into every capability directly:

- The starter's `rust/Cargo.toml` depends on five framework crates by path (`forge-runtime`, `forge-storage`, `forge-net` with `ureq`, `forge-services`, `forge-devtools`), plus `uniffi` pinned exactly and `async-trait` because UniFFI's generated code names it. A new developer starts by knowing the internal crate map.
- The library must be named `forge_ffi` and the namespace `forge_ffi`, because the build scripts and host projects expect those names ([app-crate.md](app-crate.md#rules-that-keep-it-working)). The app's identity is not in its own crate name.
- What a capability needs from the app (a data directory, a cancellation scope, permission to run in the background) is passed by hand each time, because there is no application context object for capabilities to receive.

The result is that "framework vs app" (AGENTS.md rule 1) is enforced only by convention. The framework does not dictate app structure, which is good, but it also offers none, so each app's domain code is interleaved with infrastructure it should not own.

### 3. The Swift half of the framework does not exist as a library

On the Swift side, framework code reaches an app in two ways, and neither is a dependency:

- **Copied into `Generated/` on every build.** `scripts/generate-bindings.sh` copies `ForgeLog.swift`, `ForgeImages.swift`, `ForgeAudioSession.swift` and the `ForgeServices*.swift` adapters from the checkout into `Generated/ForgeFFI/`, the same module as the UniFFI bindings. Whatever the checkout has at build time is what the app gets. `--minimal` apps receive `ForgeImages.swift` and `ForgeAudioSession.swift` although they use neither.
- **Copied into the app at `forge init`, then owned by the app.** `AppLifecycle.swift` is byte-identical in the template, Chat and Good Dog; `AppServices.swift` and `ErrorAlert.swift` likewise; `LaunchAppearance.swift` is identical in the template and in all six iOS apps (Chat, Counter, Device Lab, Good Dog, Pocket Library, Weather). A fix to any of them reaches no existing app.

The earlier assessment of what to lift from the demos into a template (2026-11-08) reached the same conclusion from the demo side: the repeated Swift (open the core off the main actor with a loading/ready/failed phase, run the event loop until `nil` and re-read on `eventsDropped`, forward Task cancellation to Rust) is framework behaviour living as app code. There is no `import ForgeKit`; the framework's Swift has no version, no tests of its own outside the apps that copied it, and no way to evolve.

### 4. Who owns the Rust instance

In every demo and both starters the Rust core is created by a view: `ContentView` holds `@State private var model = NotesModel()` and calls `model.open()` from `.task`. Lifecycle is reported from the same view with `.forgeLifecycle`, which reads `@Environment(\.scenePhase)`, a *scene's* phase.

This inverts the ownership the app needs. The Rust instance should live for the process, and its lifecycle is the *application's* (launch, background, suspension, termination), not a window's. Today it works because an iPhone app has one scene, but:

- Its lifetime is that of a view's state, which SwiftUI may recreate. `open()` is idempotent only within one `NotesModel`.
- If a second scene ever exists (iPad multitasking, an external display, a future extension), each gets its own `AppCore` on the same directory, which [app-crate.md](app-crate.md#lifecycle) says is unsupported and nothing prevents (S2's directory lock was not added).
- Work that must outlive the screen (R4) has no owner that is not a view.
- `AppServices.install()` runs in `App.init()`, but the core it serves is opened later by a view, so the order of "services installed, core opened, first phase reported" is by convention. `NotesModel.open()` already has a branch to replay a phase reported before Rust was open.

There are also three `@main` entry points per app (`AppHostXtoolApp`, `AppHostLauncher`, `Xcode/<Name>/AppMain.swift`), each repeating the scene setup, because xtool, `swift run` and Xcode want different product shapes.

### 5. The Rust–Swift boundary

What works, and should stay: commands as `async` exports, pull-based bounded event streams with drop counts, explicit operation objects for long work, file paths for large data, typed error enums. The [FFI policy](ffi-policy.md) is sound as policy.

What is missing is the *application interface* Reuben describes ("commands in, snapshots and events out"). Each app invents its own:

- **Observing state.** No common answer. The full template sends the whole list in an event *and* re-reads after each command, so the same change arrives twice by two routes. Pocket Library and Good Dog send an invalidation and the Swift model re-queries, which scales better and is the pattern to standardise (Part 2).
- **Long-running operations.** Counter and Weather have operation objects with `result()`, `cancel()`, `status()`. The template and Pocket Library pass an optional `Cancellation` per call. Chat has neither for downloads (N3's cancellation part is still open).
- **Navigating away.** Works only where the app wraps the call in `withTaskCancellationHandler` and starts it from `.task(id:)`, because Swift Task cancellation does not reach Rust (F2). Forgetting it is silent.
- **Errors.** Every app has its own enum. The host maps each case to text by hand, and the template shows `reason` (a diagnostic) to the user in `describe`. There is no shared notion of retryable, needs-user-action, corrupt or newer-data (F4).
- **Blocking calls.** Plain exports run on the caller's thread. Chat's `set_draft` saves the store on the calling thread (`chat-core` `Inner::persist`), is documented as "cheap enough to call as the user types", and a failed save is kept in `store_error` rather than returned. Nothing in the boundary distinguishes a memory read from a disk write.

### 6. Lifecycle

The `Lifecycle` primitive is correct (tested in the foundation review), and the template uses it well for one case: requests started on screen stop in the background and `resumed` is announced. But there is no *application lifecycle contract*:

| Situation | What happens today |
|---|---|
| Cold launch | The view opens the core. No step for migrations, interrupted-work recovery or a launch report; Chat's outbox replay is app code |
| Background | Foreground scope cancelled (template, Good Dog, Chat). Pocket Library and Weather do nothing in Rust |
| Must-finish work (a save, an upload) | No `beginBackgroundTask`; the process may be suspended mid-work (R4) |
| Suspended, then terminated | Nothing runs; saves before returning protect simple apps. Downloads are lost; nothing records that they were running |
| Relaunch after termination or crash | No record of what was in flight, so nothing resumes and the user is not told. Only Chat's outbox survives |
| Permission or data-protection change | Not modelled (P2, R4) |

[app-crate.md](app-crate.md) says "a new app gets it for free" of the lifecycle table. That is true of the full template only; the `--minimal` starter, which is where beginners now start (#139), removes the lifecycle entirely.

### 7. Persistence as an application guarantee

The primitives hold under crash tests (foundation review). As an application guarantee, almost nothing is defined:

| Guarantee | State |
|---|---|
| Atomic writes | Yes, per file (`write_atomic`, SQLite transactions) |
| One writer per data directory | Stated, not enforced |
| Schema versions and migrations | Version number in `JsonFile`; migration hook exists. Neither starter shows a migration; no fixtures of old files anywhere (S7) |
| Interrupted writes | Safe per file; file-plus-record is a convention (S4) |
| App upgrades | No test or convention proves version N's data opens in N+1 |
| Export and backup | Pocket Library has `export_library`; no framework notion. No file is excluded from backup or given a protection class (S5) |
| Corruption | Each app decides. Pocket Library's `open_fresh` (move aside, start empty, refuse newer) is the best design and lives in the app |
| Where data lives | Chosen in Swift by each model (`Application Support/<Name>`), passed to Rust as a string. The framework does not own the layout, so it cannot apply backup or protection rules to it |
| Sensitive data | Keychain adapter exists; no rule for what goes there; Keychain items outlive the app's data |

Reuben's question was whether an ixOS app can evolve over several years without losing user data. Today that depends entirely on each app's author.

### 8. `forge init` and what it generates

| Expectation | Generated app today |
|---|---|
| Independent of the ixOS checkout | **No.** `rust/Cargo.toml` names the checkout by absolute path (`/home/.../ixOS/crates/forge-runtime`). The Xcode project's build phase does too (`REPO_ROOT`). `forge build` runs the checkout's `scripts/`, builds `uniffi-bindgen` from it, and copies Swift adapters from it. Moving or deleting the checkout breaks the app |
| Declares its framework version | **No.** Nothing in the app records which Forge it was made with or builds against. All crates are `0.1.0` and unpublished. `Cargo.lock` pins third-party crates, but path dependencies follow the checkout's working tree, and the Swift adapters are re-copied from it on every build, so pulling the checkout silently changes every app |
| Clear application entry point | Partly. Three `@main`s; the real entry is a view's `.task` |
| Working Rust-to-Swift path | Yes, and tested |
| Lifecycle and state pattern | Full: yes, but one of five shapes (above). Minimal: neither |
| Basic persistence example | Yes (`JsonFile`), without a migration |
| Tests without an iPhone | Yes. Rust tests, `forge check swift` for the model on Linux, XCTest on macOS. This is a genuine strength |
| Can produce a distributable release | Not yet; see [first-app-store-release.md](first-app-store-release.md) (asset catalog, distribution signing, privacy manifest are all unbuilt) |

**The minimal and full starters do not share an architecture.** `--minimal` is not the full app with features turned off: it has a different state model (commands return the whole list, no events), no lifecycle, no shutdown, no services. A developer who grows a minimal app into a real one has to restructure it. Since #139 made `--minimal` the beginners' path, most new apps start on the shape the framework least supports.

### 9. Framework or collection of capabilities

A collection, with the template standing in for the framework. The test Reuben proposed, "an app should be able to use networking without independently inventing retry policies, cancellation rules, lifecycle handling and error reporting", fails today: the template's `check_url` hand-wires a foreground scope, a host cancellation link, a `background_cancellable`, and a mapping of `Cancelled` to `ShutDown` or `Interrupted` depending on which scope fired; Pocket Library repeats the wiring with a different scope; retry policy does not exist (N2).

### 10. Documented but not implemented

| Document says | Code |
|---|---|
| Counter is the reference app, with one serial command thread per instance, no lock across I/O, ordered commands ([architecture.md](architecture.md#concurrency-and-persistence)) | Only Counter does this. Neither starter, nor any demo built since, follows it |
| "A new app gets [the lifecycle] for free" ([app-crate.md](app-crate.md#lifecycle)) | Not in `--minimal`; not used by Pocket Library or Weather |
| "Fresh apps work outside the checkout" ([ideal-design.md](ideal-design.md#6-ideal-properties-inferred-product-bar)) | Outside the checkout's directory, yes; without the checkout, no |
| "No process-global mutable state" ([ffi-policy.md](ffi-policy.md)) | Already noted (R6) |
| `forge.toml` `background_modes = ["processing"]` | Accepted; nothing to call (R4) |

### Scorecard

| Area | Question | Today |
|---|---|---|
| App structure | One clear, canonical architecture? | **No.** Six shapes across two starters and five apps |
| Init | A genuinely independent application? | **No.** Absolute paths into an unversioned checkout |
| FFI | Robust and ergonomic? | Robust per call; no application interface; cancellation and errors by hand |
| Lifecycle | Correct across iOS transitions? | Primitive correct; no contract; must-finish and relaunch missing |
| State | Consistent model for state, events, operations? | **No.** Five state models |
| Persistence | Can apps evolve and recover? | Per-file safety only; upgrades, layout, backup unowned |
| Native integration | Use iOS without fighting the framework? | Mostly yes for the services that exist; Swift adapters unversioned copies |
| Dependencies | Reproducible versions? | **No** for Forge itself; yes for third-party crates |
| Testing | Most behaviour testable without hardware? | **Yes.** The strongest area |
| Distribution | Archive, sign, submit an independent app? | Not yet (planned, unbuilt) |

### What is right and should be kept

Rust owning state and rules; native SwiftUI; UniFFI with proc macros and library-mode bindgen; pull-based bounded events; async commands; `Lifecycle` scopes; `CancelHandle`; `forge.toml` as the one source of Info.plist, entitlements and `xtool.yml`; Rust and Swift-model tests on Linux; the foundation primitives; Chat's outbox as the future operations contract; Pocket Library's recovery design; Counter's command thread. The Linux-first workflow is not implicated in any finding.

### Is anything fundamentally wrong?

Three things are, in the sense that adding features on top will not fix them and each makes the next app more expensive to change later:

1. **No application runtime.** The framework has no type that is "the app", so every concern Reuben listed (state, lifecycle, operations, errors, persistence layout) is solved per app. More primitives will make this worse, not better: each is another thing to wire by hand.
2. **The Swift side is copied, not depended on.** As long as framework Swift is copied, the boundary between framework and app does not exist on half the stack, and nothing can be fixed once for every app.
3. **The framework is not a versioned dependency.** An app cannot say what it is built on, so there is no way to evolve the framework without silently changing every app, and no way to ship an app whose build is reproducible.

The foundation review's six contracts remain right; they describe what the runtime must guarantee. What they lacked was the thing that owns them. Part 2 proposes it.

---

## Part 2: the canonical ixOS application

### The shape

```
┌──────────────────────────────────────────────────────────────┐
│ Native iOS application (the app's Swift)                     │
│ SwiftUI views · navigation · @Observable models              │
├──────────────────────────────────────────────────────────────┤
│ ForgeKit (Swift package, versioned)                          │
│ ForgeHost: owns the core for the process · lifecycle ·       │
│ background tasks · event loop · cancellation · error kinds · │
│ native service adapters                                      │
├────────────── typed application interface ───────────────────┤
│ the app's #[uniffi::export] API: commands, queries, events,  │
│ operations; app's own names and types                        │
├──────────────────────────────────────────────────────────────┤
│ Application core (the app's Rust)                            │
│ domain state · rules · decisions · schema                    │
├──────────────────────────────────────────────────────────────┤
│ forge-app (Rust, versioned): the application runtime         │
│ command executor · published state + revision · lifecycle    │
│ contract · operations · data directory · error kinds         │
├──────────────────────────────────────────────────────────────┤
│ Capabilities: forge-storage · forge-net · forge-services ·   │
│ (later) operations queue · background transfers             │
└──────────────────────────────────────────────────────────────┘
```

The one new piece is the **application runtime**, present on both sides of the boundary: `forge-app` in Rust and `ForgeKit` in Swift. It is generic infrastructure that every app needs; it holds no domain. The application core is the app's own code and owns its state, rules and decisions. Capabilities receive what they need (a data directory, a scope, a priority) from the runtime instead of from hand wiring.

**The runtime stays light** (Reuben's review of this page). `forge-app` provides mechanics, not an application manager:

- It does not own the app's state type or require one. The executor runs the app's closures in order over whatever the app keeps (one struct, several stores, a SQLite handle, a game world); "published state" is a revision counter and an optional snapshot the app chooses to publish, not a framework-shaped model.
- It does not choose a persistence mechanism. JSON, SQLite, plain files or nothing at all stay the app's choice through the capability crates. The runtime provides the directories, the commit point (a command's result is reported only after the app's closure returns), and the recovery and migration *hooks*; it does not dictate a schema format.
- Every piece is usable alone. An app can take the lifecycle and operations without the executor (Chat's client already has its own threads), or the executor without events. The starter uses all of it; nothing else must.
- It does not schedule durable jobs. That waits for a second real user (N1), as below.

What this deliberately does not do: make Rust objects directly observable from SwiftUI, generate SwiftUI, impose a navigation model, or introduce a generic event bus. The app's exported API stays its own, in its own names.

### What an app writes (Rust)

Illustrative, to fix the idea rather than the API:

```rust
// The app's state and rules: plain Rust, no UniFFI, no framework types.
// How it is stored is the app's choice; here a JsonFile from forge-storage.
struct Notes { file: JsonFile<Saved>, saved: Saved }

// The typed application interface: the app's names. The runtime is generic
// over whatever the app hands it; it does not require a framework trait.
#[derive(uniffi::Object)]
pub struct AppCore { rt: forge_app::Runtime<Notes> }

#[uniffi::export]
impl AppCore {
    #[uniffi::constructor]
    pub fn open(env: forge_app::Environment) -> Result<Arc<Self>, AppError> { ... }

    /// Query: reads the published state; never blocks on a command or disk.
    pub fn notes(&self) -> Vec<Note> { self.rt.read(|n| n.newest_first()) }

    /// Command: runs on the app's serial executor, commits (saves) and only
    /// then publishes a new revision. Rejected with `ShutDown` after shutdown.
    pub async fn add_note(&self, text: String) -> Result<Note, AppError> {
        self.rt.command("add-note", move |notes| notes.add(text)).await
    }

    /// Operation: long, cancellable, scoped. Foreground scope: stops in the
    /// background and fails with a framework `Interrupted`.
    pub fn check_url(&self, url: String) -> Arc<CheckOperation> {
        self.rt.operation(Scope::Foreground, move |cx| fetch_status(cx, &url))
    }
}

// Emits set_phase, shutdown, events(), revision() and the shared types.
forge_app::export_runtime!(AppCore);
```

The runtime's `command` runs the closure on the executor and bumps the revision when it returns `Ok`; saving is the closure's own call to its store, so "commit before reporting" holds whatever the store is.

What disappears from every app: the `shut_down` flag, `ensure_running`, `parts()`, holding a mutex across a save, scope-to-error mapping, hand-made event types for drops and phases, choosing a data directory in Swift.

### What an app writes (Swift)

```swift
@main
struct NotesApp: App {
    // Owned by the App, for the process; opened once, off the main actor.
    @State private var host = ForgeHost(open: AppCore.open)

    var body: some Scene {
        WindowGroup {
            ForgeRoot(host) { core in NotesView(model: NotesModel(core: core)) }
        }
    }
}
```

`ForgeHost` (ForgeKit) opens the core with a framework `Environment` (data, caches and temporary directories it chose, build info, test-data flag), shows loading or a recovery screen by error kind, reports the *application* lifecycle (from `UIApplication` notifications, not a view's scene phase), runs must-finish work under `beginBackgroundTask`, runs the event loop and turns `Changed { revision }` into an observation tick that models re-query on. Task cancellation is forwarded by ForgeKit's operation wrapper, so `.task(id:)` cancellation reaches Rust without per-call code. One `@main` for xtool, `swift run` and Xcode where the product shapes allow it.

### The contracts

**Ownership.** One core per process, owned by `ForgeHost` at the `App` level. Views and models borrow it. Its private data directory has one writer, enforced in Rust by a lock. Data shared with extensions follows the rules under *Persistence* below, because a simple lock is not enough there.

**State.** Rust owns state. Queries read a published, committed snapshot and never block on I/O. Every committed command increments a revision and publishes `Changed { revision, areas }`; Swift models re-query what they show. Commands return only their direct result (the new item's id, a validation outcome), never the whole state. Apps may add domain events with payloads (Chat's messages) when a stream genuinely needs them; drops always produce `EventsDropped`, after which the host re-reads. This is the Pocket Library and Good Dog pattern, chosen because it is cheap over FFI, never inconsistent, and does not require a diff.

**Commands.** One serial executor per core (Counter's design, from R1): arrival order, commit-then-publish, no lock across I/O, shutdown drains accepted commands. Commands are not cancellable once accepted.

**Operations.** Anything slow or cancellable is an operation with an id, a scope, progress, `cancel()` and exactly one outcome.

**Background work, precisely.** iOS gives an app four different things, and the runtime must not blur them. Requesting background time does not make work finish, and nothing short of a persisted record makes work survive termination.

| Kind | Guarantee | Mechanism | Example |
|---|---|---|---|
| Committed before return | Done, on disk, when the command returns; survives any later suspension or kill | The command executor; the app's store writes inside the command | Saving a note, recording a training session |
| Short extra time | *Best effort*: iOS may grant some seconds after backgrounding, and may end it early. Work must be safe to abandon at any point and is cancelled on expiry | `finishing` scope over `beginBackgroundTask`; expiry cancels the scope | Finishing an upload already 90% done, flushing a log |
| Interruptible, restartable | Stops when the app leaves the screen; the app restarts it on resume if it still matters | `foreground` scope; `Interrupted` outcome; `Resumed` event | A search request, a live chat stream, a forecast refresh |
| Durable | Survives termination and relaunch; runs again until it succeeds or fails terminally, exactly once in effect | A persisted operation record replayed at launch, with idempotency keys. `BGTaskScheduler` and background `URLSession` can add *opportunistic* progress while suspended, never a deadline | Chat's outbox; a future FieldKit upload queue |

The first three are phase 1 and 2 work. The fourth is deliberately deferred: it is a contract (record, retry classification, replay hook), extracted from Chat when a second app needs it, not a general job system. `lifetime` (runs until shutdown, freezes with the process) remains for work that may simply pause while suspended.

**Lifecycle.**

| Event | Runtime does | App may hook |
|---|---|---|
| Launch | Lock data directory; run migrations; report `Opened { fresh, restored, migrated, recovered_operations }` | Seed data; replay durable work |
| Active | Nothing | Refresh |
| Background | Cancel `foreground`; ask for background time for `finishing` work (best effort; expiry cancels it) | Nothing to save: commands already committed |
| Resumed | Renew `foreground`; re-check permissions; publish `Resumed` | Restart streams, refresh |
| Suspended / killed | Nothing can run: every command has committed before returning | (none, by design) |
| Terminated | Shutdown if iOS says so; never relied on | (none) |
| Relaunch after a kill or crash | Durable operations replayed; interrupted non-durable ones reported once as `Interrupted` | Tell the user |
| Memory or storage pressure | Lower background budgets, cancel deferrable work (E1) | Drop caches |

**Errors.** Every app error maps to a framework kind: `invalid`, `not_found`, `unavailable` (offline, retry later), `interrupted`, `cancelled`, `needs_user` (permission, sign-in), `corrupt` (offer recovery), `newer_data` (never offer a reset), `internal`. ForgeKit presents the generic kinds; the app writes copy for its domain cases only (F4).

**Persistence.**

| Guarantee | How |
|---|---|
| One writer (private data) | A lock on the app's private data directory, held by the runtime for the process |
| Layout | The runtime provides the directories and their policies; the app decides what goes where and in which format: `data/` (backed up, protected), `caches/` (re-downloadable, excluded from backup), `tmp/` (swept at launch), secrets in Keychain with first-run reconciliation (S5). An app can declare more stores and classes (a large media store excluded from backup, a "complete" protection class for a journal) |
| Shared data (App Groups, extensions) | A separate, explicit `shared/` store in the App Group container, never the private directory. Two processes (the app and a widget, share or notification extension) may open it, so a per-process lock is not enough. And iOS terminates a suspended process that still holds a file lock or an open SQLite transaction in a shared container (the `0xdead10cc` termination), so long-held locks there are wrong. The contract: SQLite in WAL mode with a busy timeout and short transactions for shared records, or a single-writer rule (only the app writes; extensions append to an inbox the app drains at launch and on resume); shared locks only inside a finishing scope; change notification across processes by Darwin notification plus a revision re-read. To be designed with the first extension (spike S2 / W9), not before; nothing in phase 1 may assume one process |
| Atomic changes | A command commits its state and its staged files together (S4) |
| Schema evolution | Each store declares a version and migrations; a copy is kept before migrating; fixtures of every released schema are tests that run on Linux (S7) |
| Recovery | Corrupt data is moved aside and reported, never silently replaced; newer data is refused (Pocket Library's `open_fresh` made general) |
| Export | Optional `export()` per model, offered by ForgeKit's settings helper |
| Upgrades | A per-release device check stays (install N, add data, update to N+1) |

**Capabilities.** Declared once in `forge.toml`, which generates the service list, the Info.plist strings and the Swift adapter selection; services check their own permission (P1). They receive the runtime's context instead of process globals where practical.

### What `forge init` generates

One template, one architecture. `--minimal` becomes the same app with fewer screens and no networking, not a different design: it keeps `ForgeHost`, the executor, the revision model and the lifecycle hooks, because they cost the app author nothing once they are in the framework.

```
MyApp/
  forge.toml        [forge] version = "0.3.0"  ← the framework version the app is built on
  rust/
    Cargo.toml      forge-app = "=0.3.0" (plus the capabilities it uses, same pin), from a registry or a git tag
    Cargo.lock      committed: third-party crates are pinned too
    src/lib.rs      the app's model and its exported interface
    tests/          Linux tests, including a migration fixture
  Sources/App/      @main, views, @Observable models (Foundation + Observation only)
  Package.swift     .package(url: ".../ForgeKit", exact: "0.3.0")
  Package.resolved  committed
  Tests/            Swift model tests (Linux via forge check swift; macOS XCTest)
  uitests/          smoke test with throwaway test data
  Generated/        UniFFI bindings only
```

No absolute paths, no checkout. The `forge` binary carries its templates, build scripts and a `uniffi-bindgen` matching the pinned UniFFI, and refuses to build an app whose `[forge] version` it does not support, with the command to upgrade. Upgrading the framework is a visible change to the same version in three places (`forge.toml`, `Cargo.toml`, `Package.swift`), which `forge` checks agree, reviewed like any dependency bump. Exact pins (`=0.3.0` in Cargo, `exact:` in SwiftPM) plus committed lock files make a build reproducible; a looser range would let `cargo update` change the framework under the app.

### Testing without Apple hardware

`forge-app` ships a test harness so app tests can drive the contracts on Linux: open, run commands, move through phases, kill and relaunch against the same directory, advance time, inject offline and slow networks. With migration fixtures this covers most of what an app can break. What still needs the iPhone is unchanged from the foundation review's list (suspension timing, background task expiry, data protection, real termination), and those become one Device Lab check each, not a per-app job.

---

## Part 3: phased plan

Each phase is small enough to review on its own and ends with something running. Testing scales to the change: Linux tests for the runtime, macOS CI for Swift, and one short phone check only where a phase claims an iOS behaviour.

| Phase | What | Proves | Size |
|---|---|---|---|
| **0. Architecture experiments (strict gate)** | Three working demonstrations, not compile checks: (1) **shared UniFFI types**: a framework crate exposes the runtime contract (phase, cancellation, change events, error kinds) and an app crate uses them in its own API, with Swift that reads naturally on both sides; (2) **ForgeKit as a package**: a generated app consumes the framework's Swift as a real versioned dependency, not copied files; (3) **independent starter**: an app is generated, moved elsewhere and built with the Forge checkout absent | (1) works → `forge-app` is a UniFFI component and ffi-policy rule 1 is relaxed for it alone; fails → keep `app_shell!`-style macros and make ForgeKit generic over a protocol the app's types conform to. (2) and (3) decide how Forge is distributed. **No phase 1 work starts until all three have a result and Reuben has read it** | Days |
| **1. `forge-app` runtime (Rust)** | Executor (from Counter), published state with revision, operations with foreground/lifetime scopes, data-directory lock and layout, error kinds, `export_runtime!`, test harness. Rewrite the starter on it; `--minimal` becomes the same app trimmed | One architecture; R1 closed; Linux model tests for every contract | 2 to 3 PRs |
| **2. ForgeKit (Swift)** | `ForgeHost` at App level, application lifecycle, `beginBackgroundTask` for the finishing scope, event loop, cancellation wrapper, error presentation, native service adapters moved in from `Generated/`. Delete the copied files from the template | Framework Swift has one home and a version; F2 closed; R4's first half | 2 PRs, macOS CI |
| **3. Independent apps** | Version the framework, publish or tag it, `[forge] version` in `forge.toml`, templates and bindgen in the `forge` binary, `check-fresh-app.sh` run with no checkout present | Reuben's "genuinely independent application" | 1 to 2 PRs |
| **4. Persistence guarantees** | Storage classes and layout (S5), commit with files (S4), migration fixtures and backup-before-migrate (S7), general recovery. Foundation review contract 1, now owned by the runtime | Apps can evolve without losing data | 2 PRs; one Device Lab check (kill mid-write, update N to N+1) |
| **5. Move the apps** | Good Dog first (closest to the template), then Pocket Library, then Chat (its outbox becomes the durable scope, N1). Weather and Counter: migrate Weather; retire Counter as the reference once `forge-app` embodies its design | Every app has one shape; the framework is proven by four real apps | One PR per app |

**Ordering against other work.**

- *App Store release.* Pocket Library should move onto the runtime (phases 1 to 4) before its first submission. Shipping version 1 fixes its data layout and storage location on users' phones; changing those afterwards needs migration code, while changing them before costs nothing. R0 and R1 of [first-app-store-release.md](first-app-store-release.md) (Linux prep, the one macOS CI run) can run in parallel.
- *New demos (FieldKit and later).* Start them after phase 2, so they are built on the runtime rather than migrated onto it.
- *New-user experience milestone.* Pause the parts that polish the current starter (`forge init --minimal` tutorial); they would be rewritten in phase 1. The parts about `forge doctor` and diagnosis are unaffected.
- *Foundation review contracts.* Contracts 2 (application model) and 6 (boundary) are absorbed by phases 1 and 2; 1 (storage) is phase 4; 3 (resources), 4 (capabilities) and 5 (operations) follow, each attached to the runtime rather than to individual apps.
- *Android.* Keep it building: `forge-app` is plain Rust apart from its UniFFI component, and the Kotlin host keeps its current shape until Android is picked up again.

**Phase 0 result (2026-11-09).** All three experiments worked: shared UniFFI types across two Swift modules, ForgeKit consumed as a versioned package, and an app built with the Forge checkout deleted, on Linux and macOS including an unsigned iOS build. Details and the six things they found are in [spikes/phase0/RESULTS.md](../spikes/phase0/RESULTS.md). The shared-types design (`forge-app` as a UniFFI component) is therefore the one to build; the `app_shell!` fallback is not needed.

**Decisions for Reuben.**

1. Accept the diagnosis (an application runtime on both sides, Swift as a real package, a versioned framework) or reject parts of it.
2. If accepted, approve phase 0. **Accepted 2026-11-09**: Reuben accepted this page as the architecture review and authorised phase 0 as a strict gate, with the three experiments above; results are reported before any runtime work.
3. Whether Pocket Library's App Store release waits for phases 1 to 4 (recommended) or ships on the current shape and migrates later.
4. How Forge is distributed (phase 0b): crates.io is the most conventional and makes `cargo` do the work; git tags need no account and can start today.
