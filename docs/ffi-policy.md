# FFI policy

## UniFFI

- **Proc macros** only for this release (`uniffi::setup_scaffolding!`, `#[uniffi::export]`).
- Pin UniFFI to the workspace version in [Cargo.toml](../Cargo.toml) (`0.32.2`).
- Async exports use UniFFI's built-in support (no `tokio` feature). The Kotlin bindings then import `kotlinx.coroutines`, so Android hosts depend on `kotlinx-coroutines-core`.
- Generate Swift or Kotlin with **library mode** from the **host** `libforge_ffi.so` (`libforge_ffi.dylib` on macOS) — never from hand-edited UDL.

## Rust-first rules

Logic lives in Rust; UniFFI is an adapter around it. These rules keep it that way:

1. **Framework crates never depend on UniFFI, with one exception.** `forge-runtime`, `forge-storage` and `forge-net` are plain Rust. `forge-app` is the one UniFFI framework crate: it exports the application contract (phases, error kinds, runtime events, cancellation, the contract version) as UniFFI component `forge_app`, whose Swift lives in ForgeKit's `ForgeApp` module and which app crates use as external types ([app-model-review.md](app-model-review.md)). Otherwise only app crates and the adapter macros (`forge_runtime::app_shell!`, `forge_services::native_services!`, `forge_app::export_runtime!`) produce UniFFI code, and the macros emit it in the app's crate.
2. **No internal Rust types cross the boundary.** Each app exports its own records, enums and errors and converts to and from its domain types. A framework type never appears in generated bindings.
3. **Large data crosses as file paths or handles**, never as byte arrays in a call: attachments, audio, images, exports.
4. **High-frequency data does not go through generated bindings or event streams.** Sensor samples, audio frames and frame timing use a narrow C ABI or shared memory, decided per capability (the planned real-time audio path is the first case).
5. **Communication between machines is a separate, versioned protocol**, never an extension of the FFI. Forge Device talks to the app over its own wire protocol; logs reach a developer through the platform log or a device-protocol sink (`forge_runtime::logging`), not through UniFFI calls.

### The foreign-trait exception: native services

Rust normally never calls Swift (see *Events are pulled, not pushed* below). Native services are the one exception: `forge_services::native_services!` declares UniFFI foreign traits (`with_foreign`) that the iOS host implements in Swift (Keychain, directories, notifications, permissions, sharing, deep links). Rules for those adapters:

- They wrap one OS API each and hold no app logic.
- They never call back into Rust from inside a call, so a Rust lock held by the caller cannot deadlock.
- Slow work (a permission prompt, a notification request) is an `async` trait method; synchronous methods return promptly and never block on the main thread.
- They are installed once at launch (`ForgeServices.install()`); Rust reaches them through `native::NativeServices::installed()` and tests use `NativeServices::for_tests`.

The Android JNI init (`ForgeAndroid.init(context)`) is the only hand-written entry point outside UniFFI, because UniFFI cannot pass the app `Context`.

## Generated artifacts

- iOS output: each app's own `Generated/` folder (for example `examples/pocket-library/Generated/`), built from that app's Rust crate (`[rust] path` in its `forge.toml`), so it holds only that app's API — `ForgeFFI/forge_ffi.swift` (Swift target `ForgeFFI`) and `forge_ffiFFI/` (header + `module.modulemap`, SwiftPM system library `forge_ffiFFI`). They live in separate folders because SwiftPM rejects targets with overlapping sources.
- Android output: `examples/pocket-library-android/Generated/uniffi/forge_ffi/forge_ffi.kt` (from `examples/pocket-library-android/rust`, which links Pocket Library's core). Each also gets `org/forgemobile/ffi/ForgeAndroid.kt`, copied from `crates/forge-runtime/kotlin/`: the one hand-written JNI entry point (`ForgeAndroid.init(context)`), which exists because UniFFI/JNA cannot pass the app `Context` that the Android HTTPS certificate verifier needs. Nothing else crosses the boundary outside UniFFI.
- **Do not hand-edit** generated files.
- Regenerate with `forge bindings generate [app-dir]` (Swift for an iOS app, Kotlin for an Android one) or `FORGE_APP_DIR=<app> ./scripts/generate-bindings.sh [out] [swift|kotlin]`.
- CI runs `./scripts/check-app-crates.sh` (Swift) and `./scripts/check-bindings.sh` (Kotlin) to detect drift (see below).

## Binding drift

**What drift means.** The Swift and Kotlin files under `Generated/` are committed, so hosts build without running UniFFI. They are a copy of what UniFFI produces from the current source of the app's Rust crate. *Drift* is when the two disagree: someone changed an exported function, record, enum or error in that crate (or bumped UniFFI) and did not regenerate, or someone hand-edited a generated file. A drifted binding can still compile and then fail at runtime with a checksum or symbol mismatch, which is why it is a CI failure.

**How the check works.** For every example with a `forge.toml`, `./scripts/check-app-crates.sh` (Swift) and `./scripts/check-bindings.sh` (Kotlin; both run in `.github/workflows/rust.yml`):

1. Build the app crate's host `libforge_ffi` (`.so` on Linux, `.dylib` on macOS) with `cargo build --manifest-path <app>/<rust path>/Cargo.toml`.
2. Runs `uniffi-bindgen` in library mode into a temporary directory, once for Swift and once for Kotlin.
3. Compares byte for byte (`diff -u`) against the committed copies:
   - iOS apps: `Generated/ForgeFFI/forge_ffi.swift`, `Generated/forge_ffiFFI/forge_ffiFFI.h` and `Generated/forge_ffiFFI/module.modulemap`
   - Android apps: `Generated/uniffi/forge_ffi/forge_ffi.kt`
4. Prints the diff and exits non-zero on the first mismatch, naming the file and the command that regenerates it.

Templates do not commit bindings: `forge init` generates them into the new app, so a template cannot drift.

**When the contract changes on purpose.** From the repo root:

```bash
# 1. Regenerate both languages
./scripts/generate-bindings.sh examples/pocket-library/Generated      # Swift
./scripts/generate-bindings.sh examples/weather/Generated
./scripts/generate-bindings.sh examples/chat/Generated
./scripts/generate-bindings.sh examples/pocket-library-android/Generated kotlin
#    (or: make bindings bindings-kotlin)

# 2. Confirm the check is clean
./scripts/check-bindings.sh

# 3. Update the hand-written code that wraps the bindings
#    - the app's Swift models that call the changed API
#    - Android host and its error mapping (messageFor)

# 4. Commit the Generated/ changes in the same commit as the Rust change
```

Review the generated diff before committing: it should only contain the API you meant to change. Unexpected churn usually means a different UniFFI version (check `Cargo.lock`) or a stale `target/` build.

**Common failures.**

- *Diff in every file, no API change:* UniFFI version differs from the workspace pin. Run `cargo update -p uniffi --precise 0.32.2` or regenerate after `cargo clean --manifest-path <app>/rust/Cargo.toml`.
- *Check passes locally, fails in CI:* the committed files were regenerated but not staged. `git status examples/*/Generated`.
- *Kotlin differs, Swift does not:* only one language was regenerated. Run both commands in step 1.
- *Whitespace-only diff:* UniFFI runs `swift-format` / `ktlint` on its output when they are on your `PATH`. The committed copies are unformatted (CI has neither tool), so regenerate with them off your `PATH`.

## Types across the boundary

- Use records/enums/errors supported by UniFFI.
- Domain errors map to an FFI error enum in the app crate (the starter: `AppError`); do not expose internal `thiserror` types directly unless converted.
- Avoid passing large nested structures; prefer snapshots and explicit actions.

## Interaction model: commands, queries, events

Every app core is built on the ixOS application runtime, `forge_app::Runtime` ([app-crate.md](app-crate.md), [app-model-review.md](app-model-review.md)). The starter app (`templates/app-rust/src/lib.rs`) shows each kind; Pocket Library (`examples/pocket-library/rust`) is the full-size reference.

| Kind | Rust | Swift | Blocks? |
|------|------|-------|---------|
| Open | `#[uniffi::constructor] fn new(storage_directory)`, holding the folder with `DataDirectory` | `ForgeHost` opens it once per process, off the main thread | No |
| Command | `async fn add_note(...)` → `rt.command("add-note", \|state\| ...)` | `try await core.addNote(text:)` | No: runs on the command thread, saves, then publishes |
| Query | `fn notes()`, reading a `Published` snapshot | `core.notes()` | No: never waits for a command |
| Other work | `async fn check_url(url, cancellation)` → `rt.run("check-url", rt.foreground(...), ...)` | `try await core.checkUrl(url:cancellation:)` inside `withTaskCancellationHandler` | No |
| Events | `events()` → `RuntimeEvent`s (`Changed { revision }`, `PhaseChanged`, `EventsDropped`, `BackgroundWork`) | `ForgeHost`'s event loop; screens follow `host.changes` | No |
| Lifecycle | `set_phase(phase)`, `phase()`, `revision()`, `shutdown()` from `forge_app::export_runtime!(AppCore, rt)` | `ForgeLifecycle.attach(host)` | No |

There is no generic event bus. An event says that something changed, not what: the host re-reads the queries it shows.

## Concurrency contract

**Async calls.** UniFFI 0.32.2's built-in async support is used as is: no tokio feature, no custom executor. Each core's runtime runs commands one at a time, in the order they were sent, on its own command thread; an async export sends the command and returns a future that resolves when it has saved. The foreign side polls that future, so a Swift `await` suspends without holding a thread, and the generated async functions are `nonisolated`, so nothing Rust does runs on the main actor.

**A command is all or nothing.** It changes a copy of the state, saves it, and only then keeps it and publishes it (the starter's `State::change`). A failed command leaves the state, the revision and the events exactly as they were. Only after a successful save does the runtime raise the revision and send `Changed`.

**Queries never wait.** They read the last published snapshot (`Published::get`), so a slow save never blocks a screen.

**Events are pulled, not pushed.** UniFFI callback interfaces were considered and not used for events (native services are the one foreign-trait exception, above): they would run Swift code on Rust threads, make Rust hold Swift observers (leaks if never removed), and give no backpressure. Instead each subscription owns a bounded queue. `next()` resolves with the next event or `None` when the stream has ended. On overflow the oldest events are dropped and the next call returns `EventsDropped { count }` first; the consumer should then re-read everything. `ForgeHost` does this for every app.

**Cancellation.** In UniFFI 0.32.2, cancelling a Swift `Task` does **not** cancel or drop the Rust future (the generated `uniffiRustCallAsync` has no cancellation handler; Kotlin's does drop the future). So:

- Work that should stop when the user gives up takes a `Cancellation`, which the Swift side cancels from `withTaskCancellationHandler` (the starter's `NotesModel.checkURL`). Run it in a work scope: `rt.foreground` (stopped by the background, the host's `Cancellation` or shutdown), `rt.finishing` (should finish if the person leaves) or `rt.lifetime` ([app-crate.md](app-crate.md#lifecycle)). An HTTP request is interrupted in flight with `HttpClient::send_cancellable`.
- Commands are not cancellable once sent: they always complete and are saved. Use cancellation only for work with no side effects to undo, such as reads and fetches, never for saves.
- Dropping a Rust future (Kotlin cancellation) only abandons the result; the command still runs. Correctness never depends on futures being polled to the end.

**Late completion.** A command started from a view keeps running when the view goes away. It commits as normal, the host hears `Changed`, and every screen re-reads.

## Long-lived event streams (Chat)

Chat's `ChatCore` (`examples/chat/rust/src/lib.rs`) is the reference for a stream that outlives any single command. Its contract, which the Swift host relies on (it started as `ForgeChatApp`, shared with the Android example removed on 2026-11-15, so some names below are that API's):

- **Two separate things.** `events()` returns a `ChatEventSubscription` (pull-based like the runtime's `events()`: `async next()`, `cancel()`, bounded queue of `CHAT_QUEUE_CAPACITY` = 256, then `EventsDropped { count }`). `subscribe(conversation_id)` / `unsubscribe(conversation_id)` open and close a room, and `watch` / `unwatch` keep it live in the background (for `Notify`); the room's server connection runs while it is either. Ending an event stream never ends a connection, or the other way round.
- **Room connections are explicit commands, not tied to a future's lifetime**, because Swift does not cancel Rust futures (above) and Kotlin does. Both return at once. When the last of `unsubscribe` / `unwatch` returns, `StreamDisconnected { will_retry: false }` is queued and nothing more from that room's connection ever is. The Rust thread behind it ends promptly: ending the stream closes that room's connection, so it does not wait for a server heartbeat. `live_streams()` reports it, and the Rust tests assert it reaches 0.
- **One event stream for every room.** Hosts filter by `conversation_id`, which every event except `EventsDropped` carries. Events are totally ordered and each is queued after the store change it describes, so re-reading `cached_messages` after any event (and after `EventsDropped` in particular) is always consistent.
- **Async commands are not cancellable from the host.** Leaving a room must not cancel a send already on the wire; dropping the future only ignores the answer, the store and the event stream still get the outcome.
- **Every event, every error is typed.** `ChatError` adds `Storage`/`Internal` to the core's variants; fields are `reason`/`detail` (never `message`, which Kotlin's `Throwable` owns).

The design and the reasons are in [chat-demo.md](chat-demo.md).

## Lifecycle and ownership

- The app core is a UniFFI object (Rust `Arc`). ForgeKit's `ForgeHost` owns it for the life of the process; models are given the core, not their own copy.
- Every async call holds its own reference to the Rust object for as long as it runs, so releasing the Swift object mid-call is safe: nothing can touch a freed instance.
- One core per storage directory: `DataDirectory` holds the folder, and a second core on it fails with `InUse`.
- `shutdown()`: refuses new commands (`ShutDown`), stops running work, finishes the commands already accepted, and ends every event stream. Idempotent. Queries keep answering with the last published state.
- App suspension and termination need no special handling: a command reports success only after its save, so everything reported is on disk.
- The host reports the OS lifecycle with one synchronous call, `set_phase(phase)` (`Launching`, `Active`, `Inactive`, `Background`, `Terminated`). Rust owns the response: foreground work is stopped in the background, and `PhaseChanged { resumed: true }` arrives on the way back ([app-crate.md](app-crate.md#lifecycle)). The call does no I/O, because it runs on the UI thread.
- No process-global mutable state: everything belongs to an instance.

## Panics

- Domain code must not panic on expected validation failures (use `Result`).
- No `.expect()` on locks. Use `forge_runtime::lock`, which recovers the guarded value on poisoning, which is sound because each critical section is a single assignment that a panic cannot leave half-done.
- Unexpected panics may still become fatal Swift errors from UniFFI scaffolding — treat as bugs.

## Error categories

Each app has its own error enum, and maps every case to a framework `ErrorKind` (the starter's `app_error_kind`), so ForgeKit presents any app's errors the same way (`ForgeErrors.describe`, ForgeAppKit's `forgeAlert`):

| `ErrorKind` | Meaning | How ForgeKit presents it |
|-------------|---------|--------------------------|
| `Invalid` | The input was refused | The app's message, inline; nothing changed |
| `NotFound` | The thing no longer exists | Message; re-read |
| `Unavailable` | Something needed is down or switched off | Message; Try Again later |
| `Network` | A server or device couldn't be reached | Message with Try Again |
| `Refused` | The other side said no | Message; the person has to change something first |
| `Interrupted` | Stopped because the app left the screen | Safe to retry on return |
| `Cancelled` | Cancelled by the user or the host | Not an error: no message |
| `NeedsUser` | Needs a permission or a sign-in | Explain and offer the way to fix it |
| `Corrupt` | Saved data could not be read | Blocked screen offering recovery |
| `NewerData` | Saved data is from a newer app version | Blocked screen asking to update. **Never offer a reset** |
| `ShutDown` | The core has shut down | Reopen |
| `Internal` | A bug or an unexpected failure | Message; Try Again |

`forge_app::app_error_conversions!` writes the runtime and storage conversions for the usual cases ([app-crate.md](app-crate.md#rules-that-keep-it-working)). Expected errors never panic and never turn into success.

## Exposing a new operation

Using the starter (`templates/app-rust`, which every `forge init` app begins from) as the example. [feature-recipe.md](feature-recipe.md) walks a whole Pocket Library feature through every layer.

1. Add the rule (and a test). Validate the input before the command, so a refused input never reaches the command thread.
2. Add an `async` export that runs the change as a command. The starter's own `update_note`:

   ```rust
   // inside #[uniffi::export] impl AppCore
   pub async fn update_note(&self, id: u64, text: String) -> Result<Note, AppError> {
       let text = note_text(text)?;
       self.rt
           .command("update-note", move |state| {
               state.change(|stored| {
                   let note = stored
                       .notes
                       .iter_mut()
                       .find(|n| n.id == id)
                       .ok_or(AppError::NotFound { id })?;
                   note.text = text;
                   Ok(note.clone())
               })
           })
           .await
   }
   ```

   A query that needs no disk reads the `Published` snapshot and stays sync. Give any new error case its `ErrorKind` in `app_error_kind`.
3. Regenerate the bindings (`forge bindings generate`) and commit `Generated/`.
4. Call it from the model, which only presents (`perform` shows a failure as an alert with Try Again):

   ```swift
   private func update(id: UInt64, text: String) async -> Bool {
       await perform(retry: .update(id, text)) { core in
           _ = try await core.updateNote(id: id, text: text)
       }
   }
   ```

## Consuming it from SwiftUI

The `App` makes the host once (`makeAppHost()` in `AppHost.swift`), attaches the lifecycle, and shows the screens once the core is open. Models hold the core and re-read when the host's `changes` moves:

```swift
@Observable @MainActor
public final class NotesModel {
    public private(set) var notes: [Note] = []
    public var alert: ForgeAlert<Action>?
    public let core: AppCore

    public init(core: AppCore) {
        self.core = core
        notes = core.notes()
    }

    public func reload() { notes = core.notes() }
}

// In the view:
//   .onChange(of: changes) { model.reload() }   // changes = host.changes
//   .forgeAlert($model.alert) { action in Task { await model.handle(action) } }
```

## Testing the integration

- Rust: each app crate's tests (`cargo test --manifest-path <app>/rust/Cargo.toml`) cover commands, failed saves (no change, no event), event order, cancellation, shutdown with work in flight, and reopening. The runtime itself is tested in `crates/forge-app`, and model-based in `crates/forge-runtime/tests/foundation.rs`.
- Swift: each app's `Tests/AppHostTests` drive the model and the host against the real Rust library and generated bindings, with no Rust mocks. `forge check swift` runs them on Linux; `./scripts/test-swift-host.sh` runs the ones that need Apple frameworks on macOS.
- `./scripts/check-fresh-app.sh` creates iOS and Android apps outside the checkout and checks their own Rust crates and bindings ([project-generation.md](project-generation.md#checked-in-ci)).
- `./scripts/check-template-sync.sh` checks that the starter crate pins the workspace's UniFFI version and that every Android host uses the verifier version in `Cargo.lock`.

## Platform data

- File paths cross the boundary as strings; secure storage and Keychain belong in future platform adapters, not JSON persistence.
