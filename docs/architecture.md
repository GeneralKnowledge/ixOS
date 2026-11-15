# Architecture

Forge Mobile splits **portable Rust application logic** from **thin platform hosts** and a **typed UniFFI boundary**.

> The application model is `forge-app` plus ForgeKit ([app-model-review.md](app-model-review.md)). [architecture-assessment.md](archive/architecture-assessment.md) assesses the design's trade-offs and where it should improve.

## Framework principles

ixOS is a framework of reusable capabilities, not only a way to build Rust-backed SwiftUI apps. Two rules decide what goes into it, a third how it is used, and a fourth which side owns what.

1. **Move reusable infrastructure into ixOS, not app identity or domain logic.** Persistence, networking, file handling, logging, background work, notifications, media and device integration are machinery every app needs, and the framework maintains one version of each. What an app is and does stays in the app: Chat's conversations, Pocket Library's books and reading rules, Weather's forecasts, an assistant's personality.
2. **Extract when there is a real second use, or when the code is clearly platform-level.** A second app (or a second feature) that needs the same thing is the signal. Platform-level code, such as a wrapper over an Apple framework that any app would write the same way, can move on its first use. Code that only might be reused stays in the app until it is.

A third rule decides how the framework is used:

3. **Human first, agent optional.** Every ixOS app must be buildable, testable, debuggable and maintainable by a person with the documented tools. Capabilities live in the CLI (or a library it uses) with `--json` output; tools built on top (forge-lab's MCP server) run those commands, never their own implementation of a build, test, inspection or device operation. Agent configuration is generated on request (`forge agent setup`), never required. Deterministic work (configuration, builds, bindings, signing, device access, logs, screenshots, accessibility measurements, UI test runs, evidence) is Forge's, not an agent's, and only a person records an acceptance. [human-workflow.md](human-workflow.md) is the contract, and `scripts/check-human-workflow.sh` tests it in CI.

A fourth rule decides which side of the boundary owns what:

4. **Native first: use Rust to build an iOS app, not to own what Apple already provides.** Rust owns portable behaviour: domain rules, data, queues, retry decisions, state. Apple's frameworks own what they already do well, and ixOS wraps them rather than reimplementing them. Six consequences:

| Rule | What it means |
|---|---|
| Don't recreate Apple functionality | Prefer the native framework (WebKit, PDFKit, PhotosUI, AVFoundation, URLSession, Swift Charts, MapKit) over a custom implementation, in Rust or in Swift |
| Don't put OS-specific rules in portable Rust | Rust validates general invariants (a positive interval, a valid identifier, a coordinate in range). Platform limits, such as the shortest repeat iOS accepts, are checked by the native adapter, which returns a meaningful error. Where a limit affects portable logic, the service reports it as a capability instead of Rust hard-coding it |
| Don't put domain logic in Swift | Swift presentation models manage screen state, turn Rust events into UI updates, coordinate sheets and alerts, and handle lifecycle callbacks. Decisions that are part of the app's domain stay in Rust. Don't move presentation into Rust just to have more Rust |
| Don't duplicate native helpers in apps | A reusable operation (security-scoped copying, image downscaling) lives once in `forge-services`; apps keep their own policy (naming, staging, MIME handling) |
| Don't route UI-only platform values through Rust | A native service gets a Rust interface only when the Rust core reads its value (connectivity for download rules, say). A value only the screen shows (a compass heading, a Bluetooth state) comes from Apple's framework in Swift directly ([reinvention audit](research/reinvention-audit.md)) |
| Don't confuse a Rust abstraction with a native implementation | A portable interface (`SecureStore`, `HttpClient`) can have a platform implementation behind it: the Keychain adapter is the model |

Good examples to copy: the Keychain (`SecureStore` in Rust, Security framework in Swift), location and motion (native sensors, Rust receives values), read-aloud (AVSpeechSynthesizer speaks the text Rust structured), sharing (`ForgeShareLink` on SwiftUI's `ShareLink`) and `ForgeImages` (ImageIO, not image decoding in Rust). Where current code departs from this rule, and the plan for each, is in [native-capability-roadmap.md](native-capability-roadmap.md#where-current-code-departs-from-native-first).

In practice:

- When a demo needs something that another app could reasonably use, check the [capability ledger](capability-ledger.md) first. If ixOS has it, use it, or extend it if it falls short; do not work around it in the app. If ixOS lacks it, build it in the app, and note in the PR that it is a candidate.
- Let features work first, then extract. Extraction happens after the PR that introduced the code has merged, never by holding that PR back.
- Split a module into Rust (rules, state, queries) and native (the platform API) the same way the app crates are split. The table below says which side owns what.
- An app selects capabilities. A capability starts as a module or Cargo feature of an existing crate, and becomes its own crate only when it brings heavy dependencies or native code that an app must be able to leave out.
- Every module has a row in the ledger's [Modules](capability-ledger.md#modules) table, and its status there follows the evidence in the capability rows.

| Capability | Rust side | Native iOS side |
|---|---|---|
| Storage | Data models, queries, migrations, file bookkeeping | Keychain, protected and backup-excluded files |
| Networking | Requests, retries, queues, offline behaviour | Connectivity, background `URLSession` transfers |
| Notifications | Scheduling decisions and state | UserNotifications |
| Attachments and media | Hashing, queues, transfer logic | Photos, Files, Quick Look, ImageIO thumbnails |
| Audio | Application logic | AVFoundation, the audio session, Now Playing |
| Local AI | Model lifecycle, prompts, conversations | Foundation Models, Core ML inference |

The [framework extraction audit](framework-extraction-audit.md) applies these rules to the current demos and lists what should move, in order.

## Layers

| Layer | What lives there | Examples |
|-------|------------------|----------|
| Native apps | SwiftUI (and Compose) screens, view models, OS integration that must be native | `examples/*/Sources`, `templates/` hosts |
| Platform adapters | The thin code that joins Rust to a platform: generated UniFFI bindings, the services adapters, the Android JNI init | `Generated/`, `crates/forge-services/swift`, `ForgeAndroid.kt` |
| Portable Rust framework | Logic shared by every app, with no UniFFI and no platform code of its own | `forge-runtime`, `forge-storage`, `forge-net` |
| Independent infrastructure | Developer tooling that runs beside the app, never inside its FFI | `forge-cli`, `xtool`, Forge Device (planned) |

Each app owns one Rust crate (`[rust] path` in `forge.toml`, usually `rust/`), staged as `libforge_ffi` (its own library name is the app's, such as `notes_core`), with the Swift module `forge.toml` names (`[swift] bindings_module`: `NotesFFI` for apps from `forge init`, `ForgeFFI` in older ones) and the Kotlin package `uniffi.forge_ffi`. The crate holds the app's domain logic and its UniFFI API; the framework crates supply the plumbing. There is no shared FFI crate: two hosts share a crate only by pointing at the same path. See [app-crate.md](app-crate.md) and the rules in [ffi-policy.md](ffi-policy.md#rust-first-rules).

What every app crate gets from `forge-runtime`:

- `background` / `background_cancellable`: blocking work behind an `async` export, no async runtime.
- `EventHub` / `Subscription`: bounded, pull-based event streams.
- `Lifecycle`: the phase the host reports, with lifetime and foreground cancellation scopes.
- `app_shell!`: the UniFFI types every app repeats (its phase enum, a `Cancellation`, its event stream object), emitted under the app's own names. The logic stays in the plain Rust above; the macro only adds wrappers.
- `logging`: one log pipeline for every build (logcat, the iOS system log, recent records in memory, extra sinks for developer tools, panics logged). Logs stay in Rust and the platform log; they never stream across the FFI.

## The reference apps

Checklist (`examples/checklist`) is the smallest app on the runtime; Pocket Library (`examples/pocket-library`) is the full-size reference ([roadmap](roadmap.md), P4). Every app from `forge init` starts in the same shape ([app-crate.md](app-crate.md)):

```
SwiftUI views → model (@Observable, Foundation only) → ForgeHost (ForgeKit) → generated Swift (<App>FFI) → the app's Rust crate (rust/) → forge-app runtime, forge-storage, forge-net, forge-services
```

| Layer | Owns | Must not |
|-------|------|----------|
| SwiftUI views | Layout, ForgeUI components, task lifetimes (`.task`) | Hold app rules; call Rust directly for changes |
| Model (`@Observable`, imports Foundation and Observation only) | Presentation state, alerts (`ForgeAlert`), forwarding cancellation | Implement domain rules; import SwiftUI or UIKit |
| `ForgeHost` (ForgeKit) | Opening the core once per process off the main thread, the lifecycle, the event loop, `changes` | Hold app state |
| Generated bindings | Marshalling | Be edited by hand |
| The app's Rust crate | Commands, queries, domain rules, validation, persistence, errors with an `ErrorKind` | Know about SwiftUI |
| `forge-app` `Runtime` | The command thread, published snapshots, events, work scopes, shutdown | Know about any one app |

The Rust-to-Swift contract (async, events, cancellation, lifecycle, errors) is in [ffi-policy.md](ffi-policy.md).

## Ownership

- **Authoritative state** lives in the app's Rust core, owned by the runtime's command thread.
- Swift holds a **display copy**, re-read from the core's queries after each command and whenever the host's `changes` moves.
- Swift does **not** implement the app's rules locally.

## Persistence

- Versioned JSON (`forge_storage::JsonFile`, or `forge_app::Document` for one saved file) or SQLite (`forge_storage::Database`, with migrations). [persistence.md](persistence.md) has the guarantees and how to change a schema.
- iOS: the storage directory comes from the host (`Application Support/<App>`, or a throwaway folder under `-ForgeTemporaryData`), held with `DataDirectory` so only one core writes there.
- Android: storage directory provided at init from Kotlin (`filesDir` subfolder).
- Linux tests use temporary directories.
- Files the app owns (attachments, covers, downloaded models) go in a `forge_storage::files::FileStore` under that directory: the app saves the store's relative names, never absolute paths, and calls `sweep` at launch to drop files no record refers to.

### Concurrency and persistence

Each core has **one serial command thread**, owned by its `forge_app::Runtime`:

- Every command runs there, one at a time, in arrival order. Two commands can never interleave, so there are no lost updates and no lock is held across disk I/O.
- A command changes a copy, saves it, and only then keeps it and publishes it. A failed save leaves memory and disk as they were and announces nothing. After a successful save the runtime raises the revision and sends `Changed`: a change is never reported before it is on disk.
- Queries read the published snapshot. A read during a write returns the state before that write, immediately.
- Other slow work (a fetch, an export) runs off the command thread in a work scope (`foreground`, `finishing`, `lifetime`), and ends by sending a command if it changes anything.
- Shutdown runs every command already accepted, stops running work, ends every event stream, then stops the thread.

Saves write a temp file, `fsync` it, rename it over the old file, then `fsync` the directory so the rename survives power loss (Unix). Data from a newer build is refused, never overwritten (`NewerData`); unreadable data is reported (`Corrupt`), never silently reset.

## Build pipeline

```
cargo build (host) → uniffi-bindgen → Generated/* (Swift or Kotlin)
iOS: cargo build --target aarch64-apple-ios → build/ios-device/lib/libforge_ffi.a → swiftpm + xtool dev
Android: cargo ndk → app/src/main/jniLibs/<abi>/libforge_ffi.so → Gradle assembleDebug
```

See [toolchain-validation.md](toolchain-validation.md) for verified steps and blockers.

## Crates

| Crate | Role |
|-------|------|
| `forge-runtime` | `background`, `EventHub`, `Lifecycle`, `app_shell!`, `logging`, and the Android JNI init. No UniFFI dependency of its own; `app_shell!` emits UniFFI code in the app's crate |
| `forge-services` | Native services (Keychain, directories, notifications, permissions, sharing, deep links): `native_services!` emits UniFFI foreign traits in the app's crate, and `swift/` holds the iOS adapters. See [native-services.md](native-services.md) |
| `forge-storage` | Crash-safe `write_atomic`, `StorageError`, versioned `JsonFile` with migrations, `FileStore` for app-managed files (SHA-256 naming and verified downloads with the `hash` feature), and with the `sqlite` feature `Database` (bundled SQLite: migrations, transactions, FTS5, recovery of a damaged file). Used by Chat since M2 |
| `forge-net` | `HttpClient` trait, request/response types, `NoopClient`, `StaticClient` (test double), and with the `ureq` feature `UreqClient` (blocking HTTPS, rustls with the platform certificate verifier). `HttpClient::open_stream` reads long-lived bodies (Server-Sent Events) incrementally, and `open_stream_cancellable` lets a `CancelHandle` close the connection from another thread (interrupting a blocked read). Used by the Weather example and `chat-core` |
| `chat-core` (`examples/`) | The Chat demo's domain (exposed by the Chat app's own crate as `ChatCore`): `ChatClient` (commands, per-room SSE pumps with reconnect, one event sink), `ChatRepo` (in-memory and HTTP), JSON store with a `since` cursor. See [chat-demo.md](chat-demo.md) |
| App crates (`examples/*/rust`) | Each app's UniFFI exports, built as `libforge_ffi`. Checklist, Pocket Library, Commonplace, FieldKit, Relay, Sketchbook and Soundscape are on the `forge-app` runtime; Chat, Good Dog and Weather move to it in wave 3 ([app-parity.md](app-parity.md)) |
| `forge-cli` | Developer commands wrapping scripts; iOS toolchain calls go through the `IosBuilder` trait (`src/ios_builder.rs`, xtool backend today) |
