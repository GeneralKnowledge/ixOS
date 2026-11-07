# Architecture

Forge Mobile splits **portable Rust application logic** from **thin platform hosts** and a **typed UniFFI boundary**.

## Framework principles

ixOS is a framework of reusable capabilities, not only a way to build Rust-backed SwiftUI apps. Two rules decide what goes into it.

1. **Move reusable infrastructure into ixOS, not app identity or domain logic.** Persistence, networking, file handling, logging, background work, notifications, media and device integration are machinery every app needs, and the framework maintains one version of each. What an app is and does stays in the app: Chat's conversations, Pocket Library's books and reading rules, Weather's forecasts, an assistant's personality.
2. **Extract when there is a real second use, or when the code is clearly platform-level.** A second app (or a second feature) that needs the same thing is the signal. Platform-level code, such as a wrapper over an Apple framework that any app would write the same way, can move on its first use. Code that only might be reused stays in the app until it is.

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

Each app owns one Rust crate (`[rust] path` in `forge.toml`, usually `rust/`), built as `libforge_ffi` with the Swift module `ForgeFFI` and the Kotlin package `uniffi.forge_ffi`. The crate holds the app's domain logic and its UniFFI API; the framework crates supply the plumbing. There is no shared FFI crate: two hosts share a crate only by pointing at the same path (`examples/counter-android` uses `../counter/rust`). See [app-crate.md](app-crate.md) and the rules in [ffi-policy.md](ffi-policy.md#rust-first-rules).

What every app crate gets from `forge-runtime`:

- `background` / `background_cancellable`: blocking work behind an `async` export, no async runtime.
- `EventHub` / `Subscription`: bounded, pull-based event streams.
- `Lifecycle`: the phase the host reports, with lifetime and foreground cancellation scopes.
- `app_shell!`: the UniFFI types every app repeats (its phase enum, a `Cancellation`, its event stream object), emitted under the app's own names. The logic stays in the plain Rust above; the macro only adds wrappers.
- `logging`: one log pipeline for every build (logcat, the iOS system log, recent records in memory, extra sinks for developer tools, panics logged). Logs stay in Rust and the platform log; they never stream across the FFI.

## Counter, the reference app

**iOS**

```
SwiftUI (ContentView) → CounterViewModel → CounterBridge (CounterClient) → Generated Swift (ForgeFFI) → counter crate (examples/counter/rust) → forge-runtime, forge-storage
```

| Layer | Owns | Must not |
|-------|------|----------|
| SwiftUI view + view model (`CounterApp` / `AppHost`) | Presentation state, user messages, task lifetimes | Implement counter rules; import `ForgeFFI` |
| `CounterBridge` (thin Swift layer) | The Rust instance's lifetime, async/await adapters, event sequences, cancellation forwarding, Swift error translation, `Sendable` value types | Hold state of its own or duplicate Rust logic |
| Generated `ForgeFFI` | Marshalling | Be edited by hand |
| Counter crate, FFI modules (`api`, `app`, `session`, `events`) | Commands, queries, events, lifecycle; the serial command thread | Expose domain or store types directly |
| Counter crate, `domain` / `store` modules | Domain rules / persistence | Know about FFI or threads |

The bridge is per app: it is Counter-specific glue over the generated types. Apps from `forge init` start smaller: their own Rust crate (`rust/`, see [app-crate.md](app-crate.md)) exports a Swift-friendly API (async commands, an event stream object, typed errors), and the view model calls the generated `ForgeFFI` types directly. Add a bridge layer like Counter's when an app needs `AsyncSequence` events or extra Swift-side cancellation. The Rust-to-Swift contract (async, events, cancellation, lifecycle, errors) is in [ffi-policy.md](ffi-policy.md).

The Weather example ([examples/weather](../examples/weather/README.md)) has the same layering with `WeatherBridge` (`WeatherClient`, `LocationProvider`) over `ForgeWeatherApp`, which uses `forge-net` for HTTP. It re-exports the generated record and enum types instead of wrapping them, because exercising UniFFI's mapping of nested types is its purpose.

**Android**

```
Compose (MainActivity) → CounterViewModel → Generated Kotlin (uniffi.forge_ffi) → libforge_ffi.so → counter crate (examples/counter/rust) → forge-runtime, forge-storage
```

## Ownership

- **Authoritative counter state** lives in the counter crate's `domain` module inside `ForgeCounterApp` (Rust).
- Swift holds a **display snapshot**, updated from each command's result and from `StateChanged` events (so a change made by another component sharing the same client also shows).
- Swift does **not** implement increment/decrement/reset rules locally.

## Persistence

- Versioned JSON file (`forge-counter-v1.json`) via `JsonSnapshotStore`.
- iOS: storage directory provided at init from Swift (`Application Support/<App>`).
- Android: storage directory provided at init from Kotlin (`filesDir` subfolder).
- Linux tests use temporary directories.

### Action + persistence contract

**Policy:** A domain action commits **only if** the required `save` succeeds.

Implementation (`examples/counter/rust/src/session.rs`, run on the instance's command thread by `src/app.rs`):

1. Clone current engine state into a candidate.
2. Apply the action on the candidate (domain errors return without touching live state or disk).
3. Persist the candidate snapshot.
4. On successful save, replace the live engine with the candidate.

Failed saves leave memory and disk unchanged from the last committed state, and emit no event.

### Concurrency and persistence

Each `ForgeCounterApp` instance has **one serial command thread** that owns the engine and the store:

- Every command (and every save) runs there, one at a time, in arrival order. Two commands can never interleave, so there are no lost updates and no lock is held across disk I/O.
- After a successful save the thread publishes the committed snapshot and then emits `StateChanged`. A state change is never reported before it is on disk.
- Queries (`snapshot`, `startup_kind`, `active_operation`) read the published snapshot under a short lock. A read during a write returns the state before that write, immediately.
- A long-running operation (`start_bulk_add`) does its slow part on its own thread, so ordinary commands keep flowing, then sends **one** commit command. It is all-or-nothing: cancelled or failed before the commit, nothing changes. Its amount is applied to the state at the commit point and validated again there. One long-running operation at a time (`OperationInProgress`).
- Shutdown runs every command already accepted, cancels the running operation and waits for it, ends every event subscription, then stops the thread.

Not serialised: queries, and the slow part of a long-running operation. Opening two instances on the same storage directory at the same time is not supported (not enforced; each would overwrite the other's saves). This is **not** a multi-file transactional store — a crash during write may leave the temp file behind, but the primary snapshot file is replaced via rename after a full write.

### Startup

| Condition | Behaviour |
|-----------|-----------|
| No snapshot file | Fresh install (`StartupKind::FreshInstall`) |
| Valid snapshot | Restore (`StartupKind::Restored`) |
| Corrupt snapshot (bad JSON, empty file, out-of-range values, unknown old version) | `ForgeAppError::CorruptPersistence { reason }` — no silent reset |
| Snapshot from a newer build (`schema_version` above `SCHEMA_VERSION`) | `ForgeAppError::UnsupportedSchemaVersion { found, supported }` — valid data; hosts must **not** offer a reset |
| Read failure | `ForgeAppError::PersistenceReadFailed { reason }` |
| Missing storage directory | `ForgeAppError::StorageDirectoryUnavailable` |

Explicit recovery: Rust exports `recover_counter_app` (Swift: `recoverCounterApp(storageDirectory:)`). The SwiftUI host may wrap that in a view-model method such as `recoverFromCorruption()`. Recovery overwrites the snapshot with a fresh counter **only** when it is corrupt (or missing). Valid data is restored instead, and newer-schema or unreadable data is left untouched and returned as an error. The fresh state is written with the normal atomic replace, so there is no moment without a snapshot file.

### Schema versions and migration

The envelope carries `schema_version`. `SnapshotEnvelope::decode` reads only that field first, then:

- **Newer than this build:** `UnsupportedSchemaVersion`. The rest of the file is not parsed, since a newer build may have changed its shape.
- **Older:** runs the `migrate` hook in `examples/counter/rust/src/store.rs`, one version step at a time. The upgraded data is written back on the next successful save.

To change the schema, bump `SCHEMA_VERSION` and add a `migrate` arm for the previous version. Keep the filename (`forge-counter-v1.json`) the same, or newer builds will not find older data.

Saves write a temp file, `fsync` it, rename it over the snapshot, then `fsync` the directory so the rename survives power loss (Unix). I/O errors keep the underlying `io::Error` text in their `reason` field so hosts can log it.

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
| `forge-storage` | Crash-safe `write_atomic`, `StorageError`, versioned `JsonFile` with migrations, and with the `sqlite` feature `Database` (bundled SQLite: migrations, transactions, FTS5, recovery of a damaged file). Used by Chat since M2 |
| `forge-net` | `HttpClient` trait, request/response types, `NoopClient`, `StaticClient` (test double), and with the `ureq` feature `UreqClient` (blocking HTTPS, rustls with the platform certificate verifier). `HttpClient::open_stream` reads long-lived bodies (Server-Sent Events) incrementally, and `open_stream_cancellable` lets a `CancelHandle` close the connection from another thread (interrupting a blocked read). Used by the Weather example and `chat-core` |
| `chat-core` (`examples/`) | The Chat demo's domain (exposed as `ForgeChatApp` by `examples/chat-android/rust`, and by the iOS Chat app's own crate): `ChatClient` (commands, per-room SSE pumps with reconnect, one event sink), `ChatRepo` (in-memory and HTTP), JSON store with a `since` cursor. See [chat-demo.md](chat-demo.md) |
| App crates (`examples/*/rust`) | Each app's UniFFI exports, built as `libforge_ffi`: Counter (command thread, events, cancellable operations), Weather, Chat, Pocket Library |
| `forge-cli` | Developer commands wrapping scripts; iOS toolchain calls go through the `IosBuilder` trait (`src/ios_builder.rs`, xtool backend today) |
