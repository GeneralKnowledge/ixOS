# Half-baked work audit: the foundation pass

Reuben asked on 2026-11-18 for a technical-debt and unfinished-work audit, not another project review. This is the first pass: the foundation (persistence, runtime and lifecycle, the Rust/Swift boundary, networking and offline delivery, native services), the records that say what is proven, and the surviving consequences of the early repair PRs. Measured at main `61c9e13`. It changes no code; each finding names the smallest next step, and none of them adds framework or apps.

**In one paragraph.** This pass found no known defect that loses data, though some ways data could be lost are still untested on iOS: a write cut off by iOS, a full disk, and migrations across more than one version (H4, H11). The early repair PRs have no live consequences worth reopening. The real debt is of two kinds. First, the records lag the phone: the 17 and 18 November sittings passed a lot that `status.md` still calls unproven, so the headline "14 of 102 work on an iPhone" understated the truth (this PR moves 12 to "partly works") and hides which gaps are real. Second, a few designs were replaced by better ones without the old one being retired: Relay's chat core sits half on the runtime, two JSON storage paths and two app-shell models live side by side, and nothing stops ForgeKit's release drifting from the starter. Fix the records first, then prove the foundation through the apps already in Device Lab, then retire the duplicates.

## How to read this

Each finding has one of Reuben's three kinds:

- **A, fixed early mistake.** Repaired; listed only to show its consequences are gone. Don't reopen.
- **B, merged but never completed or validated.** Works on Linux, or partly, but was never finished or proven on the phone.
- **C, built before its requirements were understood.** Works, but left an awkward interface, a duplicate or a boundary that later work went around.

And one label for its state: **broken** (wrong today), **incomplete** (part missing), **unproven** (built, no phone evidence), **duplicated** (two ways to do one thing), **obsolete** (nothing needs it), **misleading** (a record or doc says something untrue), **acceptable** (known and fine for now, with the trigger that changes that).

## Findings

### 1. The records: built against proven

| Id | Kind | State | Finding | Evidence | Next step |
|---|---|---|---|---|---|
| H1 | B | misleading | **The ledger lags the phone.** The 17 November sittings passed Relay's 1 GB transfer with resume, Chats inside Relay (messages, documents, voice, search, notifications), trusted devices both ways, web images offline, Soundscape's lock screen and interruptions, and Device Lab's Tools; 18 November passed Relay's pairing policy. Some reached the ledger (Relay's trusted devices, AI Lab); most didn't, and none reached `device-validation.md`. Several records also lagged the code: FieldKit's charts and GPX export were recorded as not built | `device-validation.md` last changed 2026-11-16; results in the project's `iphone-session/2026-11-17-results.md`, `2026-11-17-evening-results.md`, `2026-11-18-results.md`; e.g. `full-text-search.toml` still says `iphone = "No"` though Relay's chat search passed | **Done for the ledger in this PR:** 16 capability records now cite the sitting each result came from (27 partly work, from 15; 46 unproven, from 56). The validation rows were added once Reuben confirmed them (H2) |
| H2 | B | misleading | **The validation history stopped at 16 November.** The baseline table is a snapshot of the first combined build and stays as it is (Wave 3 already records Weather, Good Dog and Chat after it). What was missing was any record of the 17 November sittings, and Chat now lives inside Relay. Sketchbook has still never been recorded on a phone | `docs/device-validation.md` (last changed 2026-11-16 before this PR) | **Done in this PR:** a "17 November sittings" section, Reuben's reports as he gave them, general impressions kept general and pending rechecks marked |
| H3 | B | misleading | **The ledger has no "tried and it failed" state.** Over-the-air installs are recorded `iphone = "No: never seen to install"` yet `status.md` lists them under "built, not yet run on an iPhone". Module rows disagree with their own records: Local AI says L1 has not run on the phone (its record says it partly passed on 2026-11-08), Logging names "the starter and Chat", Attachments still points at Chat M3 (#56) | `docs/ledger/forge-device/over-the-air-installs-forge-publish-ios.toml`, `docs/ledger/modules/*.toml`, `docs/status.md` Modules table | Module rows corrected in this PR. Still to do (a small CLI change): let `forge ledger generate` put a "No:" with evidence under its own heading ("tried, doesn't work") |
| H4 | B | unproven | **The real gap, once H1 is done.** Foundation capabilities that apps already use every day but nobody has recorded on the phone: finishing work in the background (Checklist export, Pocket Library imports), SQLite (Commonplace, FieldKit, Relay), the managed file store, backup and restore, connectivity, Keychain, background refresh. The foundation review's five phone checks (kill mid-write, permission revocation, suspension, motion rate, locked-file open) have never been run | `docs/status.md` "Built, not yet run"; `docs/foundation-review.md#checks-that-need-the-iphone` | After H1, list what is still unproven and map each to one step in an app already in Device Lab. Fold checks 1 (kill mid-write) and 3 (suspension) into the next sitting; they need a small Device Lab addition first |

### 2. Runtime and lifecycle

| Id | Kind | State | Finding | Evidence | Next step |
|---|---|---|---|---|---|
| H5 | C | duplicated | **Relay's chat core is half on the runtime.** `ChatCore` holds forge-app's `Runtime<()>` only for the data-folder lock and phases; its commands still run on 16 `background()` threads over its own `Mutex` state, which is the pattern the runtime replaced (foundation review R1). The parity rule `forge-host` says Yes because it proves adoption, not use. Its lifecycle thread caused #322's failing test: it could hold the core while the owner dropped it, so the folder stayed locked. #322 (still open) contains a fix for that race, a gate; it waits for merging, a Swift compile and its phone checks | `examples/relay/rust/src/chat/mod.rs` (`Runtime::with_directory("chat", directory, ())`, `background("chat-send", …)`); #322 test `a_shut_down_core_frees_its_folder_at_once` | An investigation item, not a broad runtime rewrite. After #322 merges and passes its checks, move `ChatCore`'s commands onto `rt.command` as part of Relay's baseline work, then tighten `forge-host` to fail on `forge_runtime::background` in an app crate |
| H6 | C | duplicated | **Two app-shell models are exported.** forge-runtime's `app_shell!`, `Lifecycle` and `background` (the model from #12 and #28) sit beside forge-app's `Runtime` (from the app-model review). `app_shell!` is now used only by Relay's chat module and the Android template, but the ledger still lists "Shared app shell" as a capability waiting for the phone | `crates/forge-runtime/src/shell.rs`; `grep app_shell!` finds `examples/relay/rust/src/chat/mod.rs` and `templates/android-rust/src/lib.rs` | After H5, keep the plain-Rust pieces forge-app uses (`Lifecycle`, `CancelHandle`, `Subscription`) and stop apps from using `app_shell!`; drop or merge its ledger row. The macro itself stays public until the Android template is moved off it, which waits until Android resumes |
| H7 | C | acceptable | **Threads are unbounded** (review R3): `background()` starts an OS thread per call, and 36 source files start threads. Nothing has run out yet | `crates/forge-runtime/src/background.rs` | Keep. Trigger: Relay with many trusted devices or a measured hitch. Moving Relay's chat onto the runtime (H5) removes most of the per-call threads anyway |
| H8 | C | incomplete | **The runtime depends on the HTTP stack** (review R5). forge-runtime takes forge-net with `ureq`, so every app links rustls, the platform verifier and, on non-iOS targets, `jni`, because `CancelHandle` lives in forge-net | `crates/forge-runtime/Cargo.toml` | Measure first, as Reuben said: build one app without network use and compare size. If it matters, move `CancelHandle` and `AbortRegistration` into forge-runtime with a re-export at the old path |
| H9 | B | incomplete | **Resume handling.** Apps watching `scenePhase` themselves is fixed by #322 (`ForgeHost.onResume`, in flight). Re-checking permissions on resume (review P2) is still open | `docs/research/everyday-capabilities.md` phase 2 | Nothing here; leave to #322 and phase 2 |

### 3. Persistence

Reuben's questions, answered from the code:

- **Can an interrupted write leave a partly valid state?** Not for JSON, SQLite or the file store under one writer: tested on Linux with repeated `SIGKILL`, and one writer per folder is enforced by `DataDirectory`. Never tried on iOS (H4, check 1).
- **Are migrations safe across several versions?** Partly. The first write in a new schema keeps the old file. Only Checklist and Pocket Library have fixtures of an old version (v1 each), and SQLite still has no Rust data-migration step (review S7). H11.
- **What happens when storage is full?** The write fails and the error says so; the old file stays. No test fills a disk. H11.
- **Is data from a newer version protected?** Yes: `JsonFile`, `Document` and `Database` all refuse a newer file and never touch it.
- **Do all apps follow the same recovery contract?** No. H10.

| Id | Kind | State | Finding | Evidence | Next step |
|---|---|---|---|---|---|
| H10 | C | duplicated | **Two ways to save an app's state as JSON.** forge-app's `Document` (change a copy, save, then keep it; never replace an unreadable file; set it aside only when the person asks) is used by Checklist, Relay, Sketchbook and Soundscape. Good Dog and Pocket Library still use `JsonFile` directly with their own save logic, written before `Document` existed. No parity rule covers it | `crates/forge-app/src/document.rs`; `examples/good-dog/rust/src/lib.rs` (`file: JsonFile<Stored>`); `examples/pocket-library/rust/src/store.rs` | Add a parity rule `document` with Good Dog and Pocket Library as listed lag, then move each with its next feature PR (Pocket Library already has its v1 fixture to prove the move) |
| H11 | B | incomplete | **Migrations and full storage are untested beyond one version.** Fixtures exist for two apps; none for Good Dog, Relay, Commonplace, FieldKit, Soundscape, Sketchbook. Nothing tests a full disk | `examples/*/rust/fixtures/` | Each app adds a v1 fixture the first time it changes its schema (a rule in `persistence.md`, not a sweep now). One forge-storage test that writes into a size-limited directory, if Linux CI can make one |
| H12 | B | incomplete | **Backup and protection classes don't exist** (review S5): no file is excluded from backup or given a protection class. The last open `gap_*` test is the hashed store (S3) | `grep isExcludedFromBackup` finds only `scripts/ai-check`; `crates/forge-storage/tests/foundation.rs` `gap_a_hash_name_always_matches_its_bytes` | Scheduled (everyday capabilities, phase 3). It matters before the first App Store release: Soundscape's downloads and Relay's received files would fill iCloud backups. S3 waits for a real user |

### 4. Networking and offline delivery

| Id | Kind | State | Finding | Evidence | Next step |
|---|---|---|---|---|---|
| H13 | C | duplicated | **Relay now has two durable queues with their own retry rules:** chat-core's outbox (server messages) and Relay's delivery outbox (trusted devices). The review's trigger for an operation contract (N1, N2) was "a second durable queue"; it has arrived, but inside one app, so the framework rule (extract on a second *app*) still says wait | `examples/relay/messaging/chat-core/src/client.rs`; `examples/relay/rust/src/outbox.rs` (forge-net's `Backoff`) | An investigation item, not a refactor. Inside Relay, when the baseline reaches "outbox as tested foundation", check whether a shared record shape and retry classification would be enough; the two queues needn't share an implementation. No framework change |
| H14 | B | obsolete | **Relay's stage 0 spike is still in the tree.** It is throwaway code, now rebuilt by M1, kept as the interop reference for the iroh version pins | `examples/relay/spike/README.md` ("Not production code"); `examples/relay/rust/Cargo.toml` comments | Delete it once an interop test runs Relay's own `relay` CLI against the app crate instead of the spike |
| H15 | A | acceptable | **Downloads, partials and network errors** (review N3, F4): fixed. `download_to_file` has a byte limit and swept partials; `ErrorKind` contract 3 has `Network` and `Refused`; ForgeKit 0.6.0 carries it | `crates/forge-net/src/client.rs`; `crates/forge-app/src/contract.rs` | None |

### 5. Native services and the Rust/Swift boundary

| Id | Kind | State | Finding | Evidence | Next step |
|---|---|---|---|---|---|
| H16 | B | incomplete | **The framework's own Swift adapters still `print`.** The `swift-logging` rule moved the apps onto `ForgeLog`, but it checks `Sources/AppHost` only; nine `print`/`NSLog` calls remain in the service adapters, so their warnings miss `forge device logs` and crash reports. The deep-link adapter no longer prints the URL (review P4, partly fixed) | `crates/forge-services/swift/ForgeServices.swift`, `optional/ForgeServices+{Http,Bluetooth,Background,Motion,Location}.swift` | Small PR: route them through the same logger and widen the rule to `crates/forge-services/swift` |
| H17 | C | misleading | **Two FFI rules are still false** (review R6, F1): `ffi-policy.md` says "no process-global mutable state" (services and logging are global), and rule 4 says high-frequency data doesn't cross generated bindings (motion delivers up to 100 Hz through a foreign trait) | `docs/ffi-policy.md` lines 17 and 142; `crates/forge-services/src/sensors.rs` `MIN_MOTION_INTERVAL_MS = 10` | Reword both rules to say what is true and why; the motion ceiling waits for review check 4 on the phone |
| H18 | A | acceptable | **Cancellation from Swift** (review F2): fixed by adoption. No app wires cancellation by hand any more (`withRustCancellation`), and the `rust-cancellation` rule keeps it that way. Still a helper, not generated, which is fine | `docs/app-parity.md` | None |

### 6. Releases and the starter

| Id | Kind | State | Finding | Evidence | Next step |
|---|---|---|---|---|---|
| H19 | B | unproven | **ForgeKit's release can drift from the starter unnoticed.** ForgeKit 0.6.0 was tagged on 2026-11-16 from `3b15315`. Since then about 960 lines have been added to `forgekit/` (`ForgeBanner`, `ForgeFeedback`, `ForgePickedFile`, accents, and soon `onResume`) without a forge-app version bump, so `forge init` outside the checkout still pins 0.6.0 while copying today's templates. Today's starter uses only symbols 0.6.0 has, so it works, but nothing checks that, and Linux CI can't compile the Swift | `forge-release.toml`; `crates/forge-cli/src/commands.rs` `framework_release`; `git diff 3b15315 HEAD -- forgekit` | Release ForgeKit 0.7 after #322 merges (the onResume contract is the natural point). Then add a check to `check-template-sync.sh` that fails when `forgekit/` public API changed since the released tag while forge-app's version is unchanged. This also protects the planned third-party developer exercise |

### 7. The early repair PRs

| Id | PR | Kind | State | What survives | Next step |
|---|---|---|---|---|---|
| H20 | #13, Android AGP 9 rolled back "for now" | A | acceptable | Android is still on AGP 8.7.3 and Gradle 8.10.2; the upgrade was never retried. With Android paused that is right. The cost of the pause is small but real: `android.yml` runs on every push to main that touches `crates/**`, and `jni` is linked into forge-runtime and forge-net on non-iOS targets (see H8) | Nothing until Android resumes; then retry AGP 9 first |
| H21 | #17, a missing `mod doctor` restored | A | acceptable | No source file in any crate or app crate is left out of its module tree today (checked by script). Nothing would catch one: the compiler ignores a file nobody declares | Optional, cheap: the same script in `make validate` |
| H22 | #89, a hand edit to the generated ledger | A | acceptable | Guarded: `forge ledger check` and the test `checked_in_ledger_matches_records` fail on any drift | None |

### 8. Docs that still teach the old design

| Id | Kind | State | Finding | Evidence | Next step |
|---|---|---|---|---|---|
| H23 | C | misleading | **Counter was retired but is still cited as current**: `ForgeCounterApp` in the Android setup page, "Counter, Weather and Chat installed" in the build table, a Counter row in troubleshooting | `docs/android-setup.md:89`, `docs/builds.md:9-11`, `docs/troubleshooting.md:41-44` | Reword to the apps that exist; keep the foundation review's historical mentions |
| H24 | B | misleading | **The foundation review's "Phase 4 decision" was never taken as a whole.** Its status block lists fixes piecemeal, and the open items (R3, R5, S3, S5, S7, N1, N2, P1, P2, P4, E1, F1, F3) are spread across three plans | `docs/foundation-review.md` status blocks | This audit's tables hold the current state of each; point the review's status block here and stop updating it |

## What to do, in order

1. **Make the records true** (H1, H2, H3). A records-only PR from Reuben's reported sittings, plus the ledger state for "tried, doesn't work". No code, no phone. After it, `status.md` shows the real gap.
2. **Prove the foundation through the apps** (H4). Map what is still unproven after step 1 to steps in apps already in Device Lab, and fold review checks 1 and 3 into the next sitting. No new apps.
3. **Finish what #322 starts** (H5, H19). After #322 merges: Relay's chat core onto `rt.command` as part of Relay's baseline, then ForgeKit 0.7 and the release-drift check.
4. **One hygiene PR** (H16, H17, H23, H24, and the `document` parity rule with its two listed lags from H10). Small, mechanical, Linux-only.
5. **Leave alone until their trigger** (H7, H8, H12, H13, H14, H20): each row says what changes that.

## What this pass did not cover

The second pass, if Reuben wants it: ForgeUI components under Dynamic Type and VoiceOver across apps; each app's own state model (where an app's logic has outgrown its first design); networking timeouts and retry consistency across apps; and the original contracts of #1 to #9 one by one.
