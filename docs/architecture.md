# Architecture

Forge Mobile splits **portable Rust application logic** from **thin platform hosts** and a **typed UniFFI boundary**.

**iOS**

```
SwiftUI (ContentView) → CounterViewModel → CounterBridge (CounterClient) → Generated Swift (ForgeFFI) → forge-ffi → forge-core → forge-storage
```

| Layer | Owns | Must not |
|-------|------|----------|
| SwiftUI view + view model (`CounterApp` / `AppHost`) | Presentation state, user messages, task lifetimes | Implement counter rules; import `ForgeFFI` |
| `CounterBridge` (thin Swift layer) | The Rust instance's lifetime, async/await adapters, event sequences, cancellation forwarding, Swift error translation, `Sendable` value types | Hold state of its own or duplicate Rust logic |
| Generated `ForgeFFI` | Marshalling | Be edited by hand |
| `forge-ffi` | Commands, queries, events, lifecycle; the serial command thread | Expose `forge-core`/`forge-storage` types directly |
| `forge-core` / `forge-storage` | Domain rules / persistence | Know about FFI or threads |

The bridge is per app: it is Counter-specific glue over the generated types. The iOS template carries an identical copy, checked by `scripts/check-template-sync.sh` in CI. The Rust-to-Swift contract (async, events, cancellation, lifecycle, errors) is in [ffi-policy.md](ffi-policy.md).

**Android**

```
Compose (MainActivity) → CounterViewModel → Generated Kotlin (uniffi.forge_ffi) → libforge_ffi.so → forge-ffi → forge-core → forge-storage
```

## Ownership

- **Authoritative counter state** lives in `forge-core` inside `ForgeCounterApp` (Rust).
- Swift holds a **display snapshot**, updated from each command's result and from `StateChanged` events (so a change made by another component sharing the same client also shows).
- Swift does **not** implement increment/decrement/reset rules locally.

## Persistence

- Versioned JSON file (`forge-counter-v1.json`) via `JsonSnapshotStore`.
- iOS: storage directory provided at init from Swift (`Application Support/<App>`).
- Android: storage directory provided at init from Kotlin (`filesDir` subfolder).
- Linux tests use temporary directories.

### Action + persistence contract

**Policy:** A domain action commits **only if** the required `save` succeeds.

Implementation (`forge-ffi/src/session.rs`, run on the instance's command thread by `forge-ffi/src/app.rs`):

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
- **Older:** runs the `migrate` hook in `forge-storage/src/json_store.rs`, one version step at a time. The upgraded data is written back on the next successful save.

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
| `forge-core` | Domain types, validation, property tests |
| `forge-storage` | `SnapshotStore` trait, JSON implementation |
| `forge-ffi` | UniFFI exports; app instance with a serial command thread, events, cancellable operations |
| `forge-cli` | Developer commands wrapping scripts and xtool |
