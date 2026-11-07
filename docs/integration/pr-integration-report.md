# Integration report: PRs #32–#36

How the five outstanding iOS framework PRs fit together, what has been verified and where, and the order to merge them. Written 2026-11-07; the branch heads below are the state after integration.

PRs #30 (cancellable requests) and #31 (`forge.toml` iOS config) were already on main and are not repeated here.

## State at a glance

| PR | What | Branch | Head | Brought up to date with main | CI |
|----|------|--------|------|------------------------------|----|
| #32 | Native iOS UI: `@Observable` starter, UI standard, simulator screenshots | `claude/project-thread-01q1aa` | — | **Merged into main** (502288e) just before integration started | Green at merge |
| #35 | Developer experience (M5) | `claude/project-thread-o8h671` | b2681d8 | Yes (merge) | Green, 9/9 |
| #36 | App lifecycle reported to Rust (M2) | `claude/project-thread-qb5xw6` | 444db42 | Yes (merge) + Android fix | Green, 9/9 (was failing: Android) |
| #34 | Native iOS services (M3) | `claude/project-thread-8yrt2p` | 0a2d3b2 | Yes (merge) | Green, 9/9 |
| #33 | Pocket Library acceptance app (M1) | `claude/project-thread-3rsowe` | fb67db1 | Yes (merge) | Green, 10/10 |
| #37 | All four combined on main, plus these docs | `claude/project-thread-mtlahv` | 44a8bce | — | Green, 10/10 (draft; merges last, docs only by then) |

Branches were brought up to date by merging main, not rebasing, because force pushes are blocked for these branches. No PR has review comments; there were no unresolved review threads to carry over.

## What each PR contributes

**#35 Developer experience.** `forge-cli` gains an iOS preflight (`ios_env.rs`, `IosBuilder::preflight`) that checks the toolchain, Apple sign-in and a connected iPhone before building; failure diagnosis (`diagnose.rs`, `CommandRunner::stream_tail`) that ends a failed step with "What went wrong" in four layers (Rust library, Swift build, signing, iPhone); `forge doctor` sign-in and device checks (`ios_device` in the JSON); `forge init MyApp` into the current directory; `forge test` from inside an app. New `docs/getting-started.md` and generated-app READMEs. Touches no template source.

**#32 Native iOS UI (merged).** The starter becomes a multi-screen SwiftUI app: `NotesModel` (`@Observable @MainActor`, open phase `loading`/`ready`/`failed`), `NoteListView` (search through Rust `search_notes`), `NoteDetailView`, `ComposeNoteView`, `SettingsView` (cancellable URL check). The Rust template gains `update_note` and `search_notes`. `docs/ios-ui.md` is the UI standard. A macOS CI job renders the starter in the simulator (review aid only).

**#36 Lifecycle.** `forge_runtime::Lifecycle` (phases, `resumed` transitions, `lifetime()`/`foreground()` cancellation scopes). The template's `AppCore` gains `set_phase`/`phase`, `AppPhase`, `AppEvent::PhaseChanged`, `AppError::Interrupted`: going to the background interrupts on-screen requests, coming back announces a resume, `Terminated` is `shutdown()`. Swift `.forgeLifecycle { }` (scene phase and `willTerminate`), Kotlin `reportLifecycle` (activity events, rotation ignored).

**#34 Native services.** `crates/forge-services` and its `native_services!()` macro, which puts Keychain (`SecureStore`), app directories and file import, local notifications, permissions, the share sheet and deep links into the app's `forge_ffi` namespace. Swift adapters ship in `crates/forge-services/swift/` and are copied into `Generated/ForgeFFI` by `generate-bindings.sh`. Every generated app installs them (`AppServices.install()`, `.receivesDeepLinks()`). A device check app (`scripts/make-services-check-app.sh`, `docs/native-services-device-check.md`).

**#33 Pocket Library.** An offline-first reading tracker made with `forge init` (`examples/pocket-library`), with its own Rust crate (45 tests): Rust-owned book model and rules (status and page transitions, progress coalescing, search and sort), JSON persistence with recovery, Open Library catalogue search and import behind a trait, EPUB metadata and cover extraction in pure Rust, covers stored relative to the storage directory, reading history, export. SwiftUI: Library, Activity and Settings tabs, book detail, forms, catalogue search. Framework fixes: `build-rust-ios.sh` records the requested `SDKROOT` (stops a rebuild on every Linux run), `scripts/check-app-crates.sh` (fmt, clippy, tests and binding drift for every `examples/*/rust`) in CI and `forge check`.

## Dependencies and shared files

- #36 depends on #32's starter for its iOS side. #32 merged first, so #36 now carries its lifecycle changes on the `@Observable` model. The model's lifecycle property is `lifecyclePhase`, because `phase` is #32's open state.
- #34 and #36 both add to `templates/app-rust/src/lib.rs` and the generated bindings. Their names do not clash (`AppPhase`, `PhaseChanged`, `Interrupted` against `SecureStore`, `NativePermission` and the rest).
- #33 consumes #30 and #31 (cancellation, `forge.toml`) and touches no templates.
- #35 is independent of the others' code; it overlaps only in docs and the README.

| File | Touched by | How it was reconciled |
|------|-----------|------------------------|
| `templates/ios-app/Sources/AppHost/NotesModel.swift` | #32, #36 | #32's model kept whole; #36's `lifecyclePhase`, `setPhase`, phase applied on open, resume refresh and `Interrupted` copy added to it |
| `templates/ios-app/Sources/AppHost/ContentView.swift` | #32, #36 | Merged cleanly: `.forgeLifecycle { model.setPhase($0) }` after `.task { await model.open() }` |
| `templates/ios-app/Tests/AppHostTests/NotesModelTests.swift` | #32, #36 | Merged cleanly; #36's three tests use helpers that #32 kept |
| `templates/ios-app/Sources/*/AppMain.swift` (3) | #34 | `AppServices.install()` and `.receivesDeepLinks()` kept |
| `templates/app-rust/src/lib.rs`, `Cargo.toml` | #32, #36, #34 | Merged cleanly |
| `templates/android-app/.../NotesViewModel.kt` and test | #36 | Fixed (see below) |
| `README.md` | #32, #35, #33 | #35's wording (xtool installs but does not launch) with #32's ios-ui link and #33's `--release` line |
| `docs/roadmap.md`, `docs/app-crate.md` | #35, #36, #34 | Merged cleanly, then updated for the integrated state |
| `.github/workflows/macos.yml` | #32, #34, #33 | All jobs and steps kept: starter screenshots, services check build, Pocket Library XCTest, device build and screenshots |
| `scripts/ios-screenshots.sh`, `scripts/pocket-library-screenshots.sh` | #32, #33 | Now share `scripts/ios-simulator.sh`, as #33 planned |
| `templates/ios-app/README.md`, `forge init` next steps | #35 vs #32 | Name the screens folder and `ContentView.swift` as the root, not a single screen |
| `crates/forge-cli/src/check.rs`, `Makefile`, `rust.yml` | #33 | No overlap after #35 |

## The #36 Android failure

CI failed in `NotesViewModelTest.resumeIsReportedAndTerminateClosesTheCore` with `IllegalStateException`. Reproduced exactly on Linux: the generated Android app's view model, its tests and the generated UniFFI Kotlin were compiled on a plain JVM against the app's host Rust library, with small stubs for the four Android framework classes the view model uses. The stack trace was the same: `AppCore.addNote` → "AppCore object has already been destroyed".

**Cause: an invalid test assumption about Kotlin object lifetime.** `model.setPhase(TERMINATED)` calls the view model's `close()`, which calls `core.shutdown()` and then `core.close()`. `close()` frees the Kotlin wrapper, and UniFFI's Kotlin refuses any later call on it with `IllegalStateException` before Rust is reached, so Rust's `ShutDown` could never be returned. The lifecycle, `EventHub`, event subscription and Rust shutdown were all correct: Rust's `terminated_is_shutdown` passes, and the Swift twin of the test passes because Swift frees the object only when the last reference goes.

**The same rule exposed a real bug.** `addNote` and `delete` re-read the list from the instance they started with after suspending in Rust. If the app terminated while a command was in flight, that instance had been freed and the view model threw `IllegalStateException`. They now re-read the current core. A new test, `terminatingWhileACommandIsInFlightDoesNotCrash`, failed before the fix and passes after.

Changes on #36:
- The terminate test now checks what terminate promises: the stream subscribed earlier receives `PhaseChanged(TERMINATED, false)` and then ends, and the model refuses further work quietly.
- `terminatedRefusesWorkInRust` keeps the `ShutDown` check by terminating the core directly, without freeing it.
- `terminatingWhileACommandIsInFlightDoesNotCrash` covers the crash.

Android CI on #36 is green after the fix (all 11 view-model tests, the generated APK and the examples).

## What is verified, and where

| Check | Where | Result |
|-------|-------|--------|
| `cargo fmt`, clippy `-D warnings`, `cargo test --workspace`, native rebuild regression, template sync, binding drift | Linux (cloud), each PR branch and the combined branch | Pass |
| App-owned crates (Pocket Library: fmt, clippy, 45 tests, Swift binding drift) | Linux, #33 and combined | Pass |
| Fresh app outside the checkout (`forge init --path`, its crate's tests, bindings) | Linux, every branch, and CI | Pass |
| Services check app generated from the combined templates (its Rust tests) | Linux, combined | Pass |
| Generated Android app view-model tests (11) | Linux plain-JVM run (combined), and Android CI on #36 | Pass |
| Swift host tests (XCTest against the real Rust), unsigned Xcode device builds, simulator screenshots | macOS CI | Green on every PR branch and on the combined branch (#37) |
| Android: JVM tests and APKs | Android CI | Green on every PR branch and on #37 |
| xtool builds on Linux with the patched xtool (1.24.0-ixos.1), iOS SDK recorded 26.0 | Reuben's laptop | Built: starter (#32 branch), services check (#34 branch, before integration), Pocket Library (#33 branch, before integration), Counter, Weather, Chat |
| Install and run on an iPhone | Physical iPhone | **Not yet recorded** for any of these apps (installs were starting when this was written) |

## What remains unverified

- Nothing from #32–#36 has been run on a physical iPhone yet. [iphone-acceptance.md](iphone-acceptance.md) is the single runbook for it.
- The combined branch has not been built with xtool on Linux; the laptop builds above used the PR branches as they were before integration.
- #35's Apple error wording in `diagnose.rs` is partly from memory; real failures may read differently.
- `xtool dev run` installs but does not launch the app. Everything that says "run" means install, then open from the home screen.
- Android: no physical device (the phone is under repair). Native services have no Android implementations (iOS is the priority).
- Pocket Library does not yet report the lifecycle (#36) or use the native services (#34); it was built before them and does not need them to pass its acceptance steps.

## Recommended merge sequence

1. **#35** (no template changes; mergeable now).
2. **#36**, then merge main into #34 and #33.
3. **#34**.
4. **#33**.

5. **#37** last. Once the four are in, its diff is only these docs and the roadmap and acceptance updates.

After each merge, merge main into the remaining branches and let CI run. Every conflict was resolved once already, either on the PR branch or on #37 (the README line in #33 is the only one left, and #37 shows its resolution).

Nothing has been merged or deleted. Each merge needs Reuben's approval.
