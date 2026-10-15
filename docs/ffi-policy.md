# FFI policy

## UniFFI

- **Proc macros** only for this release (`uniffi::setup_scaffolding!`, `#[uniffi::export]`).
- Pin UniFFI to the workspace version in [Cargo.toml](../Cargo.toml) (`0.28.3`).
- Generate Swift with **library mode** from the **host** `libforge_ffi.so` — never from hand-edited UDL.

## Generated artifacts

- Output directory: `examples/counter/Generated/` (and template copy).
- **Do not hand-edit** `forge_ffi.swift`, `forge_ffiFFI.h`, or `forge_ffiFFI.modulemap`.
- Regenerate with `./scripts/generate-bindings.sh` or `forge bindings generate`.
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
- Unexpected panics may become fatal Swift errors from UniFFI — treat as bugs.

## Platform data

- File paths cross the boundary as strings; secure storage and Keychain belong in future platform adapters, not JSON persistence.
