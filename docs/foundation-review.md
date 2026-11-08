# ixOS foundation review

Review of the ixOS platform base at `91fa41c` (main, 2026-11-08), done as the "ixOS Foundation Review" Reuben asked for: an independent audit of the code, not of the roadmap. The code was treated as the source of truth; where a document and the code disagree, this page says so. Forge tooling (CLI, device tools, UI testing, MCP) is mapped but not audited in depth, because the question is whether the application platform is dependable.

**Verdict in one paragraph.** ixOS does not need a rewrite. The individual primitives are good and, where this review could test them on Linux, correct: event streams, cancellation scopes, the lifecycle state machine, atomic JSON writes and SQLite transactions all held under model-based tests and repeated `SIGKILL`s. What is missing is the layer that turns primitives into a platform: nothing in the framework owns *work* (who runs it, in what order, with what priority and budget), nothing models the *fate of the process* (must-finish work, suspension, relaunch recovery), and nothing owns *data as a whole* (one writer per directory, files and records committed together, backup and protection classes). Today each app answers those questions itself, and the answers differ. One storage bug is serious enough to fix before anything else: a locked SQLite file is treated as corrupt, and the user's data is moved aside (S1).

**Status (follow-up PR, 2026-11-08).** Fixed: S1, S2, S6, N3, R2 and P3 (marked **Fixed** below); their tests now run in the normal suite. S3 is still an open `gap_*` test. Everything else is unchanged and waits for the Phase 4 decision.

Contents: [Phase 1, the foundation as built](#phase-1-the-foundation-as-built) · [Phase 2, tests](#phase-2-testing-the-foundations) · [Phase 3, findings](#phase-3-findings) · [Phase 4, roadmap options](#phase-4-roadmap-options) · [Where Lean helps](#where-lean-helps-and-where-it-does-not) · [iPhone checks](#checks-that-need-the-iphone)

## Phase 1: the foundation as built

### Crates and what they really do

| Crate | Lines (Rust) | Depends on (workspace) | Role in practice |
|---|---|---|---|
| `forge-runtime` | 1.5k | `forge-net` (with `ureq`, `rustls`, `jni`) | `background` (one new OS thread per call), `EventHub`/`Subscription`, `Lifecycle` with two cancellation scopes, `app_shell!`, logging pipeline (process global), Android JNI init |
| `forge-storage` | 1.4k | none | `write_atomic`, `JsonFile` (versioned envelope, migrations), `FileStore` (relative names, temp-then-rename, sweep, SHA-256 naming), `Database` (bundled SQLite, WAL, migrations, move-aside recovery) |
| `forge-net` | 2.5k | none | `HttpClient` trait, `UreqClient` (blocking, rustls with platform verifier), streaming, `CancelHandle` (the runtime's cancellation primitive lives here), `Backoff`, `download_to_file` |
| `forge-services` | 3.6k + 2.3k Swift | none (UniFFI via macro in the app crate) | `native_services!` foreign traits (Keychain, directories, notifications, permissions, deep links, sharing) always installed; optional `connectivity`, `device_status`, `motion`, `location`, `bluetooth`, `audio`; `Feed`/`Watch` fan-out for host values; process-global install slots |
| `forge-devtools`, `forge-devproto` | 0.8k, 1.3k | runtime; none | In-app development server (loopback, paired, Noise-encrypted), only with the app crate's `devtools` feature |
| `forge-device`, `forge-crash`, `forge-uitest`, `forge-mcp`, `forge-cli` | 2.1k, 1.9k, 10.9k, 4.4k, 16.6k | tooling only | Host-side tooling; never linked into an app |

About 9k lines are platform; about 36k are tooling. That ratio is the clearest evidence for Reuben's point that Forge has had more attention than the platform.

### How the pieces couple

- **Runtime depends on networking.** `forge-runtime` pulls in `forge-net` with the full HTTPS stack, for two reasons: `CancelHandle` lives in `forge-net`, and the Android certificate-verifier init is a runtime JNI export. Every app links `ureq` and `rustls` whether it uses the network or not, and the most basic runtime primitive (cancellation) is owned by the transport crate. (Finding R5.)
- **Storage and networking both write files, three ways.** `FileStore` stages writes as `.partial-<pid>-<n>`, `download_to_file` stages them as `.<name>.part`, and Chat stages downloads as `.download-<id>`. Only the first is ever cleaned up. (Finding N3.)
- **The capability model is in three places.** Permission strings and entitlements in `forge.toml`; optional service selection in the Rust `native_services!(...)` call; the Swift adapters copied according to that call. The core services are always installed. The CLI warns, but does not fail, when they disagree. (Finding P1.)
- **The concurrency model is in two places.** Counter, the documented reference app, owns a serial command thread (`examples/counter/rust/src/app.rs`) with strong ordering guarantees. The template and Good Dog use `background()` plus a `Mutex` held across the save. The framework provides neither as a contract. (Finding R1.)

### Documentation against code

| Document says | Code does |
|---|---|
| "No lock is held across disk I/O" and commands run "in arrival order" (architecture.md) | True for Counter only. The starter template and Good Dog hold the state lock during `JsonFile::save`, and commands race for the lock in no particular order |
| "No process-global mutable state: everything belongs to an instance" (ffi-policy.md) | Native services (`INSTALLED` slots), optional services and the logging pipeline are process globals. Reasonable choices, but the rule as written is false |
| "High-frequency data does not go through generated bindings" (ffi-policy.md rule 4) | Motion samples (up to 100 Hz) and location fixes arrive through UniFFI foreign-trait sink calls, and Rust listeners run on the host's callback thread |
| "Corrupt snapshot … no silent reset" (architecture.md) | Holds for `JsonFile::load`. `JsonFile::save` silently replaces a corrupt file, and `Database::open` replaces a file it merely could not check (S1) |
| Device services provide memory, thermal, battery and storage status | Polled values and change notifications for thermal and power only; no memory-warning, protected-data or storage-pressure events |
| `forge.toml` accepts `background_modes` such as `processing` | No adapter for `BGTaskScheduler`, `beginBackgroundTask` or background `URLSession` exists; declaring the mode gives the app nothing to call |

Everything else checked (event stream semantics, lifecycle table, cancellation hooks, atomic write durability, SQLite migration atomicity, verified downloads, adapter version check) matches its documentation.

## Phase 2: testing the foundations

New tests, all on Linux, all in the normal `cargo test --workspace` run unless marked as a gap:

| File | What it checks | Result |
|---|---|---|
| `crates/forge-runtime/tests/foundation.rs` | Model-based property tests (proptest): `EventHub` against a reference queue model (512 random sequences of subscribe, publish, next, cancel, drop, close, capacities 1 to 4); `Lifecycle` against the documented phase table and scope rules; random cancellation trees; concurrent hook registration during `cancel`; `Backoff` bounds and cancelled waits | Pass. Both models were checked by mutation: changing the overflow rule or the resume rule makes them fail with a shrunk counterexample |
| `crates/forge-storage/tests/foundation.rs` | Kills a child process (`SIGKILL`) 12 times in the middle of writes and checks what a relaunch finds: a `JsonFile` with a 360 KB document; SQLite with two-row transactions; a `FileStore` plus a `JsonFile` record list written in the documented order, followed by `sweep` | Pass: the document always loads whole; SQLite always reopens as `Existing`, passes its integrity check and never keeps half a transaction; no record ever names a missing or partial file, and the sweep leaves exactly the named files |
| same, `gap_*` (ignored) | S1, S2, S3 below | Failed when the review was written; S1 and S2 are fixed and now run in the normal suite, S3 is still ignored |
| `crates/forge-services/tests/foundation.rs` | `Feed` starts the host subscription once and stops it with the last listener; R2 below | Pass; R2 is fixed and its test runs in the normal suite |

Run the remaining gap test with `cargo test -p forge-storage --features sqlite,hash --test foundation -- --ignored`. When a fix lands, remove its `#[ignore]`.

What these tests do **not** show: power loss (that depends on the device honouring `fsync`), iOS data protection, suspension, jetsam, and anything in Swift. Those are listed under [checks that need the iPhone](#checks-that-need-the-iphone) or are Swift CI work. Nothing here changes the capability ledger: a Linux test is not device evidence.

Measured on the review machine (release build): `JsonFile::save` costs about five times the write it protects, because it reads and parses the existing file first. 3.0 ms against 1.2 ms at 360 KB, 26 ms against 5 ms at 3.7 MB, 310 ms against 56 ms at 38 MB.

## Phase 3: findings

Categories, as Reuben defined them: **Missing** (fundamental missing capability), **Harden** (existing capability needing hardening), **Boundary** (incorrect abstraction or ownership boundary), **Docs/verify** (documentation or verification gap), **App** (should remain app-specific). Severity is about user data and correctness, not effort.

### A. Application runtime

**Answer to "a coherent runtime or a collection of utilities?"** A collection of good utilities. The primitives compose correctly, but no part of the framework owns work or knows what the OS is about to do to the process.

| Id | Category | Finding | Evidence | Direction |
|---|---|---|---|---|
| R1 | Boundary | There is no framework command model. Counter's serial command thread is app code; the starter and Good Dog run each command on a fresh thread and serialise on a `Mutex` held across the save. Commands sent A then B can apply B then A, queries block for the length of a save, and every app that starts from the template inherits this | `templates/app-rust/src/lib.rs` (`background(...)` then `lock(&state)` then `file.save`), `examples/good-dog/rust/src/lib.rs` `change` | One framework executor per app instance (serial command queue, published snapshot for queries, commit-then-publish), extracted from Counter, which already proves the design |
| R2 | Harden. **Fixed** for `Feed`: `Watch::cancel` waits for a delivery under way, and a listener may cancel its own watch. The wider point (scoped publishers) stays open | "Cancelled work cannot continue producing events" is not a runtime contract. `Feed::publish` calls listeners outside its lock, so a listener runs after its `Watch::cancel()` has returned. More broadly, nothing ties an `EventHub` publish to a `CancelHandle`: whether cancelled work stops publishing is up to each app | `gap_no_value_reaches_a_listener_after_its_watch_is_cancelled` fails | Make `Watch::cancel` wait for an in-flight delivery (or check a per-listener flag under the delivery), and give scoped work a publisher that is closed with its scope |
| R3 | Missing | No resource model. `background()` spawns an unbounded OS thread per call; Chat adds a thread per room and an outbox thread. There is no pool, no limit, no priority, no mapping to iOS quality-of-service, and nothing that lets foreground work overtake background work | `crates/forge-runtime/src/background.rs`; 20 thread spawn sites in the core crates and `chat-core` | A small bounded executor with two or three priority classes, and budgets that the pressure signals (E below) can lower |
| R4 | Missing | The lifecycle has no notion of work that must finish or survive. Going to the background cancels the foreground scope; everything else simply freezes when iOS suspends the process. There is no `beginBackgroundTask` wrapper (finish a save or upload in the time iOS grants), no `BGTaskScheduler` adapter, no "data protected / unavailable" phase. Chat's outbox is the only work that survives termination, and it is app code | Searched the repo: no `beginBackgroundTask`, `BGTaskScheduler` or protected-data API in Swift or Rust; `forge.toml` validates `processing` mode anyway | Add a third scope, "finishing", backed by `beginBackgroundTask` with an expiration that cancels it; treat relaunch recovery as part of the lifecycle contract (see N1) |
| R5 | Boundary | The runtime depends on the HTTP stack, and `CancelHandle` (a runtime primitive) is owned by `forge-net` | `crates/forge-runtime/Cargo.toml` | Move `CancelHandle` and `AbortRegistration` into the runtime (or a tiny crate both use) and move the Android verifier init behind `forge-net`'s own feature |
| R6 | Docs/verify | The process-global rule in ffi-policy.md is false (services, logging). Two app instances in one process share services; tests that install services must not run in parallel | `crates/forge-services/src/lib.rs` `INSTALLED` | Rewrite the rule to name the globals and why they are global |

### B. Data ownership and persistence

Answers to Reuben's questions, from the code:

- *What owns a piece of data?* Each app instance, by convention. "Opening two instances on the same directory is not supported (not enforced)". Nothing locks a storage directory.
- *Crash halfway through a write?* Safe for one writer: JSON and SQLite both survive `SIGKILL` (tested). Not safe for two writers (S2), and SQLite's recovery misreads a lock as damage (S1).
- *Files and records consistent?* Only if the app follows "file first, record second, sweep at launch". Under that discipline it holds across kills (tested); nothing in the API enforces the order.
- *Backup, restore, exclusion?* Nothing. No file is excluded from backup and no file gets a protection class; there is only the app-wide `data_protection` entitlement. Keychain items survive deleting the app, so a reinstall starts with old secrets and no data.
- *Migrations?* Atomic for SQLite (SQL steps in one transaction) and stepwise for JSON. No data-migration hook in Rust for SQLite, no backup before migrating, no fixtures convention.
- *Corrupt or partial data?* SQLite: moved aside and recreated (too eagerly, S1). JSON: reported as corrupt on load, silently replaced on save (S6). Downloads: verified by hash in Chat; leftover partials are never removed (N3).

| Id | Category | Finding | Evidence | Direction |
|---|---|---|---|---|
| S1 | Harden, **severe**. **Fixed**: only `SQLITE_CORRUPT`, `SQLITE_NOTADB` or a failed quick check count as damage; the check waits 2 s for a lock, and anything else is an error | `Database::open` treats any failure of its `quick_check` as damage, including `database is locked`. Another connection (a second instance, a future app extension, a file iOS has not unlocked yet) makes it move the user's data aside and start an empty database, reported as `Opened::Recovered` | `gap_a_locked_database_is_not_recovered` fails with `reason: "database is locked"` | Recover only on `SQLITE_CORRUPT`/`SQLITE_NOTADB` or a failed integrity check; give the check a busy timeout; return lock, permission and I/O failures as errors; cap the moved-aside copies |
| S2 | Harden. **Fixed**: each write has its own temporary, and the next write removes ones a killed process left | `write_atomic` uses one fixed temporary name per target, so two writers share it. With two threads writing the same file, 167 of 400 writes failed, and the window for publishing a half-written temporary exists | `gap_concurrent_atomic_writes_never_tear` | Unique temporary names, plus an advisory lock on the storage directory held by the app instance (which also enforces "one instance per directory") |
| S3 | Boundary | `FileStore` has no notion of a content-addressed store. `write` and `accept` accept a 64-hex name without hashing and replace an existing file silently; `get` never verifies; there is no `verify`. A name that looks verified can point at other bytes, and bit rot is served as the original | `gap_a_hash_name_always_matches_its_bytes` | A separate `HashedStore` type: entries immutable, created only by `import_hashed` or `accept_verified`, with `verify(name)` |
| S4 | Missing | No cross-store commit contract. The file-then-record-then-sweep protocol works (tested) but is a convention; `FileStore` does not sync the directory after its rename (unlike `write_atomic`), so after a power cut a committed record can name a file whose rename was lost; and `sweep` racing an in-flight write deletes it | `crates/forge-storage/src/files.rs` `place`, `sweep` | Make the protocol an API: stage a file, get a token, commit the token with the record; sync directories; document when `sweep` may run |
| S5 | Missing | No backup, restore or protection policy, in Rust or Swift. Which folders are backed up, excluded or protected is left to each app and today no app sets anything. Keychain secrets outlive the app's data | Searched: no `isExcludedFromBackup` or `FileProtectionType` anywhere; `KeychainSecureStore` uses `AfterFirstUnlockThisDeviceOnly` | Storage classes declared per store (user data, re-downloadable, cache, secret) that map to backup exclusion and protection; first-run reconciliation of Keychain items |
| S6 | Harden. **Fixed**: `save` re-reads the file only before the first write or when its size or modification time changed; it refuses a corrupt file, and `save_over_corrupt` replaces one on request. Saving now costs serialising plus the write: 1.9 ms at 360 KB and 113 ms at 38 MB, from 3.0 ms and 310 ms | `JsonFile::save` reloads and parses the whole file before every write (about 5x the cost, measured above), and silently replaces a file it could not parse | `crates/forge-storage/src/json_file.rs` `save` | Remember what `load` found; refuse to save over a corrupt file unless the app recovers explicitly |
| S7 | Harden | Migration support is minimal: SQL-only steps, no Rust data-migration hook, no backup before a migration, no fixture tests of old files; moved-aside databases are never cleaned up | `database.rs` `migrate`, `move_aside` | Migration hooks with fixtures from each released schema; a copy before migrating; a cap on recovered copies. Fixtures prove "version N's file opens in N+1" on Linux; they do not replace a release's TestFlight update test (install N, add data, update to N+1, data survives), which stays a device check per release |

### C. Security and capability boundaries

| Id | Category | Finding | Evidence | Direction |
|---|---|---|---|---|
| P1 | Boundary | The capability model is split between `forge.toml`, the Rust `native_services!` list and the copied Swift adapters, and a mismatch is only a warning. There is no runtime boundary: Rust may call any installed service, and `PermissionStatus::allows_use` is advice nobody has to check. The core services (Keychain, notifications, permissions, deep links, sharing, files) are in every app | `crates/forge-cli/src/app_config.rs` (warning), `forge_services::native_services!` | One manifest in `forge.toml` that generates the service list and the Info.plist strings, with mismatches as errors; services that check their own permission before use |
| P2 | Missing | No revocation model. Permissions are polled; nothing notices a change. On iOS, turning off most privacy permissions in Settings terminates the app, so "check at launch and on resume" covers those, but notifications, provisional authorisation and reduced location accuracy change without a relaunch | `PermissionProvider` has `status` and `request` only | Re-check permissions on every `resumed` transition inside the framework and publish changes; confirm which changes terminate the app on the iPhone (check 2 below) |
| P3 | Harden. **Fixed** in part: `--release` and `--devtools` are refused together; the on-device indicator and bundle-id advice stay open | Development tooling can reach production data. `forge build ios --release --devtools` is accepted; a development build of Chat installs over the same bundle id and container as the copy in daily use, so probes and commands see real data; pairing is trust-on-first-use in a 120-second window | `crates/forge-cli/src/main.rs` (no conflict between the flags), `crates/forge-devtools/src/lib.rs` | Refuse `--release` with `--devtools`; a visible on-device indicator while a session is paired; recommend the sandbox bundle id for devtools builds of real apps |
| P4 | Harden | Deep links are parsed, not authorised, and the docs give no rule that a link-triggered action needs the user's confirmation. The Swift adapters log with `print`, and the deep-link adapter prints the whole URL, which can carry user content or tokens (AGENTS.md rule 5) | `ForgeServices+DeepLinks.swift` | Route Swift adapter logs through `ForgeLog` with the URL redacted; document "links navigate, never act" |
| P5 | App | Which data is sensitive, and what an app does with a link, are app decisions. The framework's job is the mechanisms in S5 and P1 | | |

### D. Offline operation and synchronisation

| Id | Category | Finding | Evidence | Direction |
|---|---|---|---|---|
| N1 | Missing | No operation contract. Chat's outbox is a complete design (durable `Sending` state, client-chosen ids so the server deduplicates, backoff, replay after relaunch), and it is the only one. A second offline app would copy it | `examples/chat-core/src/client.rs` | Extract the *contract*, not a sync engine: a durable operation record (idempotency key, attempts, next attempt, terminal failure), a retry classification, a replay-at-launch hook, and a reconciliation callback the app implements. Do it when the second user arrives (FieldKit or Relay), with Chat migrated onto it |
| N2 | Harden | Retry is only `Backoff`: no attempt limit or deadline, no `Retry-After`, no classification of which `NetError`s are worth retrying, no idempotency key on `Request` | `crates/forge-net/src/backoff.rs`, `types.rs` | A `RetryPolicy` beside `Backoff` that the operation contract uses |
| N3 | Harden. **Fixed** except cancellation: `download_to_file` takes a byte limit (Chat passes 25 MiB) and uses `.partial-<pid>-…` names; `FileStore::sweep_temporaries` removes them, and Chat calls it when a client opens | Downloads have no size limit, no resume, and partials nobody removes. `download_to_file` writes `.<name>.part`; Chat downloads to `.download-<id>`, so a killed download leaves `..download-<id>.part`. `FileStore::sweep` only removes `.partial-*`, and `usage()` skips hidden files, so the leftovers are invisible. Chat also passes a fresh `CancelHandle`, so an attachment download cannot be cancelled | `crates/forge-net/src/client.rs` `download_to_file`, `examples/chat-core/src/client.rs` | Downloads go through `FileStore::partial_path` with a byte limit and the caller's cancel scope; one temporary-file convention |
| N4 | Missing | No background transfers: an upload or download stops when iOS suspends the app (planned as W2/S6) | | Background `URLSession` adapter, after N1 defines what a transfer is |
| N5 | App | Conflict resolution and the meaning of synchronisation state are domain decisions (Chat's message status, a FieldKit record's merge rule). The framework supplies the queue and the hook, never the merge | | |

### E. Resource management

| Id | Category | Finding | Evidence | Direction |
|---|---|---|---|---|
| E1 | Missing | Pressure is observable but not actionable. `device_status` reports thermal state, low power, battery, storage and memory, and `defer_heavy_work` gives a sound default policy, but nothing listens for memory warnings or storage pressure, and nothing feeds pressure into scheduling (R3) | `ForgeServices+DeviceStatus.swift` observes thermal and power notifications only | Pressure events from the host into the runtime; the executor lowers background budgets and cancels a "deferrable" scope under pressure |
| E2 | Harden | Storage growth is unmanaged in places: recovered databases (S7), hidden partials (N3), and `FileStore::usage` not counting hidden files | | Count and cap everything the framework writes |

### F. The Rust/Swift boundary

| Id | Category | Finding | Evidence | Direction |
|---|---|---|---|---|
| F1 | Docs/verify | Rule 4 ("high-frequency data does not go through generated bindings") is already broken by motion, which delivers up to 100 Hz through foreign-trait sink calls and runs Rust listeners on the host's thread | `crates/forge-services/src/sensors.rs` (`MIN_MOTION_INTERVAL_MS = 10`), `ForgeServices+Motion.swift` | Measure the cost on the iPhone (check 4) and either set a stated ceiling for foreign-trait sinks or move motion to the narrow path planned for audio |
| F2 | Boundary | Swift `Task` cancellation does not reach Rust (UniFFI 0.32), so every app forwards it by hand with a `Cancellation` object and `withTaskCancellationHandler`. Correct when done, easy to forget | ffi-policy.md, `app_shell!` | Generate the Swift wrapper for cancellable calls instead of documenting the pattern |
| F3 | Docs/verify | Re-entrancy and threading rules for foreign traits ("never call back into Rust inside a call") are not checked anywhere, and `Feed` listeners take app locks on Swift threads | | A Swift host test (macOS CI) that calls back into Rust from a sink and proves no deadlock; a `loom` test for `Feed` |
| F4 | Boundary | There is no shared error taxonomy. Counter's six categories (domain, persistence, corruption, newer data, cancelled, internal) are a good design that each app re-creates under its own names, so a host cannot treat "newer data, never offer a reset" uniformly | ffi-policy.md error table, per-app error enums | A framework error category (retryable, user-actionable, corrupt, newer-data, cancelled, internal) that app errors map into |
| F5 | Docs/verify | Large data, object ownership and long-running operations are handled well and match the policy (paths not bytes, `Arc` objects held by each async call, explicit operation objects). No change needed beyond F1 to F4 | | |

### Already good enough (keep)

Pull-based bounded events with drop counts; `CancelHandle` hooks that interrupt blocked I/O; the lifecycle's lifetime and foreground scopes; `write_atomic` with directory sync; SQLite with WAL, atomic migrations and refusal of newer schemas; verified downloads; relative names in `FileStore`; native services as thin foreign traits with an interface version check; the capability ledger's separation of evidence levels; Counter's command thread design. Chat's outbox is good and should become the template for N1, not be replaced.

### Should remain app-specific

Room streams, reconnect policy and unread counts (Chat); EPUB handling (Pocket Library, until a second user); training programs (Good Dog); attachment size limits and id format (Chat); conflict resolution rules (N5); what counts as sensitive data (P5).

## Phase 4: roadmap options

Three ways forward. They are not exclusive; the recommendation combines them.

**Option A: fix in place.** Fix S1, S2, S6, N3, R2 and P3, un-ignore their tests, change no contracts. About two small PRs. Removes the data-loss risks, leaves the structural gaps.

**Option B: six platform contracts.** Turn Reuben's foundation table into six contracts, each delivered as a short spec in `docs/`, an executable model or property test, and device evidence in the ledger:

1. *Storage*: one writer per directory (lock), error classification (S1), storage classes for backup and protection (S5), the file-and-record commit API (S4), a hashed store (S3), migration hooks with fixtures (S7).
2. *Application model*: the per-instance command executor from Counter (R1), the "finishing" scope and relaunch recovery (R4), capability checks inside services (P1).
3. *Resources*: a bounded executor with priorities (R3) fed by pressure events (E1).
4. *Capabilities*: one manifest, generated service list and Info.plist, revocation on resume (P1, P2).
5. *Operations*: the durable operation contract and retry policy (N1, N2), with Chat moved onto it; background transfers after that (N4).
6. *Boundary*: rules made true (F1, R6), generated Swift cancellation (F2), shared error categories (F4), host re-entrancy test (F3).

**Option C: define the platform first.** Write the one-page definition Reuben proposed ("Rust provides the application runtime, native hosts provide operating-system integration, Forge provides development, verification and deployment") as a spec with the contracts above as its chapters, before any code. Clarifying, but on its own it produces paper.

**Recommendation.** Do Option A's storage fixes now (S1 first, it can lose user data), then Option B in the order storage, application model, resources, capabilities, operations, boundary, with Option C's definition as the half-page preface to the Option B spec rather than a separate phase. Keep the six-demo sequence; each contract should land when the demo that needs it does (operations with FieldKit or Relay, resources with Soundscape and local AI) unless it is a correctness fix. Pause new native integrations until contracts 2 and 4 exist, as Reuben suggested.

A proposal for the ledger, keeping evidence levels distinct: add a **Contract** column ("a model or property test of the documented contract exists and passes") between *In an app* and *iPhone*. It records what this review's tests prove without letting a Linux test stand in for device evidence.

## Where Lean helps, and where it does not

Lean earns its place where the property quantifies over *every* interleaving of crashes or retries in a protocol small enough to model, and where tests can only sample. Two such protocols:

1. **The storage commit protocol (S4).** "For every point at which the process can die, and every order in which renames reach the disk, a committed record never names a missing or unverified file, and the sweep never removes a named one." Ten to twenty states; the kill tests above sample this, a proof covers it, including the power-loss reorderings Linux cannot simulate.
2. **The operation contract (N1).** "With at-least-once delivery, client idempotency keys and replay after relaunch, each operation takes effect exactly once on the server, and a terminal failure is reported exactly once." This is the property Chat relies on today without stating it.

Where Lean does **not** help:

- The lifecycle, event-hub and cancellation-tree properties are finite and now covered exhaustively enough by the proptest models in this PR.
- Concurrency bugs like R2 belong to `loom` (exhaustive interleavings of real Rust code), not to a separate model that can drift from the code. Add `loom` tests for `EventHub`, `CancelHandle` and `Feed`.
- The FFI boundary is generated code and Swift runtime behaviour; the right evidence is Swift host tests and device runs.

Keep any Lean models in `docs/research/` or a `verification/` folder outside the Cargo workspace, with a CI job only once a proof exists.

## Checks that need the iPhone

None of these need doing before the Phase 4 decision; they are candidates for the coordinated sitting once a build exists. Each needs a small addition to the Device Lab app first (laptop work, done before asking for the phone).

| Check | Phone steps | What it proves |
|---|---|---|
| 1. Kill mid-write on iOS | Tap "Start write loop" (JSON and SQLite), swipe the app away from the switcher, reopen; five times | The Linux `SIGKILL` results hold on APFS and iOS termination |
| 2. Revocation | With location and notifications granted, turn each off in Settings while the app is in the background, then return to it | Which permission changes terminate the app and which do not; the basis for P2 |
| 3. Suspension | Start a 60-second job, press the side button to lock, unlock after 30 seconds | Whether foreground-scoped work stops, whether lifetime-scoped work freezes or dies, what `resumed` reports; the basis for R4 |
| 4. Motion rate | Run motion at 100 Hz for 60 seconds with the devtools session open | CPU and dropped samples through the foreign-trait path; the basis for F1 |
| 5. Locked-file open (only if S1's fix needs device proof) | Lock the phone during a background refresh with `data_protection = "complete"` | That the fixed `Database::open` reports an error, never a recovery, when iOS holds the file closed |
