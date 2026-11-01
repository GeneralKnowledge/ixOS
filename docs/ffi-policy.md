# FFI policy

## UniFFI

- **Proc macros** only for this release (`uniffi::setup_scaffolding!`, `#[uniffi::export]`).
- Pin UniFFI to the workspace version in [Cargo.toml](../Cargo.toml) (`0.32.2`).
- Async exports use UniFFI's built-in support (no `tokio` feature). The Kotlin bindings then import `kotlinx.coroutines`, so Android hosts depend on `kotlinx-coroutines-core`.
- Generate Swift or Kotlin with **library mode** from the **host** `libforge_ffi.so` (`libforge_ffi.dylib` on macOS) — never from hand-edited UDL.

## Generated artifacts

- iOS output: `examples/counter/Generated/` — `ForgeFFI/forge_ffi.swift` (Swift target `ForgeFFI`) and `forge_ffiFFI/` (header + `module.modulemap`, SwiftPM system library `forge_ffiFFI`). They live in separate folders because SwiftPM rejects targets with overlapping sources.
- Android output: `examples/counter-android/Generated/uniffi/forge_ffi/forge_ffi.kt`.
- **Do not hand-edit** generated files.
- Regenerate with `./scripts/generate-bindings.sh [out] [swift|kotlin]` or `forge bindings generate --language …`.
- CI runs `./scripts/check-bindings.sh` to detect drift.

## Types across the boundary

- Use records/enums/errors supported by UniFFI.
- Domain errors map to `ForgeAppError` in `forge-ffi`; do not expose internal `thiserror` types directly unless converted.
- Avoid passing large nested structures; prefer snapshots and explicit actions.

## Interaction model: commands, queries, events

| Kind | Rust export | Swift (`CounterBridge`) | Blocks? |
|------|-------------|-------------------------|---------|
| Open | `async open_counter_app(dir, mode)` | `try await CounterClient.open(storageDirectory:recoverCorruptData:)` | No: loads on the command thread |
| Command | `async apply_action(action)` | `try await client.apply(.increment)` | No |
| Long command | `start_bulk_add(amount, step_delay_ms)` → `CounterOperation` (`async result()`, `cancel()`, `status()`) | `try await client.bulkAdd(100)` | No |
| Query | `snapshot()`, `startup_kind()`, `active_operation()` | `client.currentState()`, `client.startupKind`, `client.activeOperation` | No: reads the last committed state, never disk |
| Events | `subscribe()` → `CounterEventSubscription` (`async next()`, `cancel()`) | `for await event in try client.events()` | No |
| Lifecycle | `async shutdown()`, `is_shut_down()` | `await client.shutdown()` | No |

The sync `ForgeCounterApp(storageDirectory:)`, `apply(action:)` and `recoverCounterApp` stay for existing callers (the Android host, JVM tests). They go through the same command thread and block the caller; do not call them from the main thread in new code.

Events (`CounterEvent`) are small and explicit: `StateChanged { snapshot }`, `OperationProgress { operation_id, completed, total }`, `OperationFinished { operation_id, outcome }` (exactly one per operation; `Completed`, `Cancelled` or `Failed { error }`), and `EventsDropped { count }`. There is no generic event bus.

## Concurrency contract

**Async calls.** UniFFI 0.32.2's built-in async support is used as is: no tokio feature, no custom executor. Each Rust instance runs its commands on its own serial command thread; an async export sends the command and returns a future that resolves when the thread answers. The foreign side polls that future, so a Swift `await` suspends without holding a thread, and the generated async functions are `nonisolated`, so nothing Rust does runs on the main actor. See [architecture.md](architecture.md#concurrency-and-persistence) for ordering and persistence.

**Events are pulled, not pushed.** UniFFI callback interfaces were considered and not used: they would run Swift code on Rust threads, make Rust hold Swift observers (leaks if never removed), and give no backpressure. Instead each subscription owns a bounded queue (`QUEUE_CAPACITY` = 64). `next()` resolves with the next event or `None` when the subscription has ended. On overflow the oldest events are dropped and the next call returns `EventsDropped { count }` first; the consumer should then re-read `snapshot()`. On the Swift side `CounterEvents` is an `AsyncSequence` whose `next()` awaits Rust directly, so that backpressure is kept (an `AsyncStream` fed by a pump task would buffer without limit or drop silently).

**Subscription lifetime.** A subscription belongs to whoever called `subscribe()`. It ends when the consumer cancels it (Swift: cancelling the iterating task), when it is released, or when the instance shuts down (queued events are delivered first, then the loop ends). Each subscription supports one consumer at a time. To avoid duplicates, subscribe once per owner: the view model's `open()` is idempotent, so a reappearing view does not subscribe again. `subscription_count()` exposes the number of live subscriptions for tests.

**Cancellation.** In UniFFI 0.32.2 (and 0.28.3 before it), cancelling a Swift `Task` does **not** cancel or drop the Rust future (the generated `uniffiRustCallAsync` has no cancellation handler; Kotlin's does drop the future). The bridge therefore forwards cancellation explicitly:

- `bulkAdd` uses `withTaskCancellationHandler` to call `CounterOperation.cancel()`. Before the commit point the operation stops (promptly, even mid-step), nothing is changed, Rust returns `Cancelled`, and Swift throws `CancellationError`. After the commit point a cancel does nothing and the committed state is returned.
- Short commands (`apply`) are not cancellable once sent: they always complete and are saved. If the task is already cancelled, the bridge sends nothing and throws `CancellationError`.
- Event iteration: cancelling the iterating task cancels the subscription, which ends the pending `next()`.
- Dropping a Rust future (Kotlin cancellation) only abandons the result; the command still runs or the operation still finishes. Correctness never depends on futures being polled to the end.

**Late completion.** An operation started from a view keeps running in Rust when the view or view model goes away. It commits (or not) as normal, its outcome is delivered to any live subscriber as `OperationFinished`, and the next open sees the saved state. The view model captures itself weakly, so a late result is simply dropped. A host that wants cancel-on-disappear calls `cancelBulkAdd()` from `.onDisappear`.

## Lifecycle and ownership

- `ForgeCounterApp` is a UniFFI object (Rust `Arc`). The Swift `CounterClient` owns it; share the client, not the generated object.
- Every async call holds its own reference to the Rust object for as long as it runs, so releasing the Swift object mid-call is safe: nothing can touch a freed instance.
- Create one instance per storage directory. Opening two at once on the same directory is not supported.
- `shutdown()`: refuses new commands (`ShutDown`), cancels the running operation, finishes the commands already accepted, waits for the operation, ends every subscription, stops the thread. Idempotent. Queries keep answering with the last committed state. Releasing the last reference does the same without waiting.
- App suspension and termination need no special handling: a command reports success only after its save, so everything reported is on disk. An operation killed with the process never committed, so nothing half-done is saved.
- No process-global mutable state: everything belongs to an instance.

## Panics

- Domain code must not panic on expected validation failures (use `Result`).
- No `.expect()` on locks. `forge-ffi` locks recover the guarded value on poisoning, which is sound because each critical section is a single assignment that a panic cannot leave half-done.
- Unexpected panics may still become fatal Swift errors from UniFFI scaffolding — treat as bugs.

## Error categories

| Category | `ForgeAppError` | Swift `CounterError` | Swift handling |
|----------|-----------------|----------------------|----------------|
| Domain | `WouldUnderflow`, `WouldOverflow`, `InvalidInput { reason }` | `.belowMinimum`, `.aboveMaximum`, `.invalidInput` | Inline message; state unchanged |
| Persistence | `PersistenceFailed`, `PersistenceReadFailed` (both carry the I/O error in `reason`), `StorageDirectoryUnavailable` | `.saveFailed`, `.readFailed`, `.storageUnavailable` | Message; blocked startup or save failure |
| Corruption | `CorruptPersistence` | `.corruptData` | Blocked UI; open again with `recoverCorruptData: true` (`recoverFromCorruption()` in the view model) |
| Newer data | `UnsupportedSchemaVersion` | `.newerDataVersion` | Blocked UI asking the user to update the app. **Never offer a reset**: the data is valid, and recovery refuses to overwrite it anyway |
| Cancelled | `Cancelled` | `CancellationError` (not a `CounterError`) | Not an error: no message |
| Lifecycle / resource | `ShutDown`, `OperationInProgress` | `.closed`, `.busy` | Reopen / wait |
| Internal | `InternalFailure { reason }` | `.internalFailure` | Message; retry |
| Platform (Swift only) | — | `.platform` | E.g. Application Support could not be created |

`CounterError.userMessage` is the text to show; `debugDescription` adds the diagnostic `reason`. Expected errors never panic and never turn into success.

## Exposing a new operation

1. Add the rule to `forge-core` (and a test).
2. In `forge-ffi`, add a method to `AppCore` (`app.rs`) that sends a job to the command thread, then an `async` export on `ForgeCounterApp` (`api.rs`):

   ```rust
   // app.rs
   pub(crate) async fn rename(&self, name: String) -> Result<CounterSnapshot, ForgeAppError> {
       let (responder, reply) = Responder::new();
       self.send(Box::new(move |worker: &mut Worker| {
           responder.respond(worker.apply(CounterAction::Rename(name)));
       }))?;
       reply.wait().await
   }

   // api.rs, inside #[uniffi::export] impl ForgeCounterApp
   pub async fn rename(&self, name: String) -> Result<CounterStateSnapshot, ForgeAppError> {
       self.core.rename(name).await.map(|s| app::to_ffi(&s))
   }
   ```

   A query that needs no disk reads `self.shared` directly and stays sync. Map new error cases in `errors.rs`, and add them to both `CounterError.init` (Swift) and the Android `messageFor`.
3. Regenerate bindings (`./scripts/generate-bindings.sh` and the Kotlin variant) and commit them.
4. Wrap it in `CounterClient`, translating errors:

   ```swift
   public func rename(_ name: String) async throws -> CounterState {
       try Task.checkCancellation()
       return CounterState(try await bridgedAsync { try await app.rename(name: name) })
   }
   ```
5. Copy the bridge change to `templates/ios-app/Sources/CounterBridge/` (CI checks they match).

## Consuming it from SwiftUI

```swift
@MainActor final class CounterViewModel: ObservableObject {
    @Published private(set) var count: Int64 = 0
    private var client: CounterClient?

    func open() async {
        guard client == nil else { return }
        do {
            let client = try await CounterClient.open(storageDirectory: directory)
            let events = try client.events()          // subscribe first…
            count = try client.currentState().count   // …then read, so nothing is missed
            self.client = client
            Task { [weak self] in
                for await event in events {
                    guard let self else { return }
                    if case let .stateChanged(state) = event { self.count = state.count }
                }
            }
        } catch { /* show (error as? CounterError)?.userMessage */ }
    }

    func increment() async {
        do { count = try await client?.apply(.increment).count ?? count }
        catch is CancellationError {}
        catch { /* inline message */ }
    }
}

// In the view:
//   .task { await model.open() }
//   Button("Increment") { Task { await model.increment() } }
```

## Testing the integration

- Rust: `cargo test -p forge-ffi` covers async commands, concurrent commands, read-during-write, save failures (no commit, no event), event ordering, overflow, cancellation, shutdown with work in flight, and reopen. Tests use an in-memory store that can fail or pause saves.
- Swift: `./scripts/test-swift-host.sh` (macOS) runs `CounterClientTests` (the bridge) and `CounterViewModelTests` against the real Rust library and generated bindings; no Rust mocks. CI runs it for the example and for a freshly generated template app.
- `./scripts/check-template-sync.sh` checks that the template's Swift matches the example.

## Platform data

- File paths cross the boundary as strings; secure storage and Keychain belong in future platform adapters, not JSON persistence.
