# FFI policy

## UniFFI

- **Proc macros** only for this release (`uniffi::setup_scaffolding!`, `#[uniffi::export]`).
- Pin UniFFI to the workspace version in [Cargo.toml](../Cargo.toml) (`0.28.3`).
- Generate Swift or Kotlin with **library mode** from the **host** `libforge_ffi.so` (`libforge_ffi.dylib` on macOS) — never from hand-edited UDL.

## Generated artifacts

- iOS output: `examples/counter/Generated/` — `forge_ffi.swift`, headers, modulemap.
- Android output: `examples/counter-android/Generated/uniffi/forge_ffi/forge_ffi.kt`.
- **Do not hand-edit** generated files.
- Regenerate with `./scripts/generate-bindings.sh [out] [swift|kotlin]` or `forge bindings generate --language …`.
- CI runs `./scripts/check-bindings.sh` to detect drift.

## Types across the boundary

- Use records/enums/errors supported by UniFFI.
- Domain errors map to `ForgeAppError` in `forge-ffi`; do not expose internal `thiserror` types directly unless converted.
- Avoid passing large nested structures; prefer snapshots and explicit actions.

## Lifecycle

- `ForgeCounterApp` is a UniFFI **object** (reference type). Mutations use interior `Mutex` — Swift calls methods on a shared instance.
- No process-global mutable singleton for app state.

## Panics

- Domain code must not panic on expected validation failures (use `Result`).
- FFI layer avoids `.expect()` on mutex locks; poisoned locks map to `ForgeAppError::InternalFailure`.
- Unexpected panics may still become fatal Swift errors from UniFFI scaffolding — treat as bugs.

## Error categories

| Category | Examples | Swift handling |
|----------|----------|----------------|
| Domain | `WouldUnderflow`, `WouldOverflow` | Inline error message; state unchanged |
| Persistence | `PersistenceFailed`, `PersistenceReadFailed` (both carry a `reason` with the I/O error), `StorageDirectoryUnavailable` | Message; blocked startup or save failure |
| Corruption | `CorruptPersistence` | Blocked UI; call `recoverCounterApp(storageDirectory:)` (often wrapped as `recoverFromCorruption()` in the view model) |
| Newer data | `UnsupportedSchemaVersion` | Blocked UI asking the user to update the app. **Never offer a reset**: the data is valid, and `recoverCounterApp` refuses to overwrite it anyway |
| Newer data | `UnsupportedSchemaVersion` | Blocked UI asking the user to update the app. **Never offer a reset**: the data is valid. `recoverCounterApp` refuses to overwrite it anyway |
| Internal | `InternalFailure` | Message; retry |

## Platform data

- File paths cross the boundary as strings; secure storage and Keychain belong in future platform adapters, not JSON persistence.
