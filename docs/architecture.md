# Architecture

Forge Mobile splits **portable Rust application logic** from a **thin SwiftUI host** and a **typed UniFFI boundary**.

```
SwiftUI (CounterApp)
    ↓
CounterViewModel (observable snapshot only)
    ↓
Generated UniFFI Swift (ForgeFFI)
    ↓
forge-ffi (adapter, persistence orchestration)
    ↓
forge-core (domain rules)
    ↓
forge-storage (JSON snapshot trait)
```

## Ownership

- **Authoritative counter state** lives in `forge-core` inside `ForgeCounterApp` (Rust).
- Swift holds a **display snapshot** updated after each successful or rejected Rust call.
- Swift does **not** implement increment/decrement/reset rules locally.

## Persistence

- Versioned JSON file (`forge-counter-v1.json`) via `JsonSnapshotStore`.
- iOS: storage directory provided at init from Swift (`Application Support/Counter`).
- Linux tests use temporary directories.

## Build pipeline

```
cargo build (host) → uniffi-bindgen → Generated/*
cargo build --target aarch64-apple-ios → build/ios-device/lib/libforge_ffi.a
swiftpm + xtool dev → signed iOS app
```

See [toolchain-validation.md](toolchain-validation.md) for verified steps and blockers.

## Crates

| Crate | Role |
|-------|------|
| `forge-core` | Domain types, validation, property tests |
| `forge-storage` | `SnapshotStore` trait, JSON implementation |
| `forge-ffi` | UniFFI exports, `Mutex`-backed app instance |
| `forge-cli` | Developer commands wrapping scripts and xtool |
