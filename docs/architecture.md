# Architecture

Forge Mobile splits **portable Rust application logic** from **thin platform hosts** and a **typed UniFFI boundary**.

**iOS**

```
SwiftUI (CounterApp) → CounterViewModel → Generated Swift (ForgeFFI) → forge-ffi → forge-core → forge-storage
```

**Android**

```
Compose (MainActivity) → CounterViewModel → Generated Kotlin (uniffi.forge_ffi) → libforge_ffi.so → forge-ffi → forge-core → forge-storage
```

## Ownership

- **Authoritative counter state** lives in `forge-core` inside `ForgeCounterApp` (Rust).
- Swift holds a **display snapshot** updated after each successful or rejected Rust call.
- Swift does **not** implement increment/decrement/reset rules locally.

## Persistence

- Versioned JSON file (`forge-counter-v1.json`) via `JsonSnapshotStore`.
- iOS: storage directory provided at init from Swift (`Application Support/<App>`).
- Android: storage directory provided at init from Kotlin (`filesDir` subfolder).
- Linux tests use temporary directories.

### Action + persistence contract

**Policy:** A domain action commits **only if** the required `save` succeeds.

Implementation (`forge-ffi/src/session.rs`):

1. Clone current engine state into a candidate.
2. Apply the action on the candidate (domain errors return without touching live state or disk).
3. Persist the candidate snapshot.
4. On successful save, replace the live engine with the candidate.

Failed saves leave memory and disk unchanged from the last committed state. This is **not** a multi-file transactional store — a crash during write may leave the temp file behind, but the primary snapshot file is replaced via rename after a full write.

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
| `forge-ffi` | UniFFI exports, `Mutex`-backed app instance |
| `forge-cli` | Developer commands wrapping scripts and xtool |
