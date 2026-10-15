# Forge Mobile — ideal design (from documentation)

This document captures how Forge Mobile **should** work according to the product README and `docs/` only. It was written **before** reading `crates/`, `examples/`, `scripts/`, or CI configuration.

Each statement is tagged:

- **[stated in docs]** — explicit in README or a doc under `docs/`
- **[my inference]** — reasonable extension of documented goals, not literally quoted

---

## 1. Architecture and crate responsibilities

### Layering **[stated in docs]**

The stack is: SwiftUI host → view model (display snapshot) → generated UniFFI Swift → `forge-ffi` adapter → `forge-core` domain → `forge-storage` persistence trait/implementation (`docs/architecture.md`).

### `forge-core` **[stated in docs]**

- Owns **authoritative domain rules** (e.g. counter transitions, validation).
- Must be testable on Linux without Apple tooling (`README.md`, `docs/architecture.md`).
- Must not depend on Swift, SwiftUI, or UniFFI-generated types **[my inference]** from “portable Rust application logic” and crate layout in `README.md`.

### `forge-storage` **[stated in docs]**

- Provides a narrow persistence interface (`SnapshotStore` trait) with a JSON snapshot implementation (`docs/architecture.md`).
- Uses a **versioned** snapshot file naming pattern (`forge-counter-v1.json`) **[stated in docs]**.
- Linux tests use temporary directories (`docs/architecture.md`).

### `forge-ffi` **[stated in docs]**

- UniFFI-facing API only; maps domain/storage to FFI types (`docs/architecture.md`, `docs/ffi-policy.md`).
- Orchestrates persistence with domain actions per documented contract (`docs/architecture.md`).
- Exposes a **reference-type** app object (`ForgeCounterApp`) with interior synchronization (`Mutex`), not a global singleton (`docs/ffi-policy.md`).
- Proc-macro UniFFI only; pinned workspace UniFFI version; library-mode bindgen from host `cdylib` (`docs/ffi-policy.md`).

### `forge-cli` **[stated in docs]**

- Developer-facing commands: `doctor`, `init`, `test`, `bindings generate`, `build ios`, `devices`, and wrappers for install/run (`README.md`, `docs/ios-signing.md`).
- Wraps **scripts and xtool**, not a replacement toolchain (`docs/architecture.md` table: “Developer commands wrapping scripts and xtool”).

### Rust / Swift boundary **[stated in docs]**

- Boundary sits at **generated UniFFI + small adapter** in `forge-ffi`.
- Swift passes **storage directory path** as string at init; no domain rules in Swift (`docs/architecture.md`, `docs/ffi-policy.md`).
- Swift holds observable **display** state updated from Rust results after each call **[stated in docs]** for ownership; view model does not re-implement validation **[stated in docs]**.

---

## 2. Developer workflow

Documented happy path **[stated in docs]**:

1. **Environment** — Install Rust, Swift, xtool, usbmuxd; `xtool setup` with Xcode.xip for Darwin SDK (`docs/linux-setup.md`).
2. **Validate** — `./scripts/validate-toolchain.sh` or `forge doctor` (`README.md`, `docs/linux-setup.md`).
3. **Test (Linux)** — `cargo test --workspace` or `forge test` (`README.md`).
4. **Bindings** — `./scripts/generate-bindings.sh` or `forge bindings generate` (`README.md`, `docs/ffi-policy.md`).
5. **Build iOS** — `forge build ios --project …` and/or `./scripts/package-ios-library.sh` then `xtool dev` in app dir (`README.md`, `docs/linux-setup.md`, `docs/project-generation.md`).
6. **Device** — `forge devices`, `forge install ios`, `forge run ios` as thin xtool wrappers (`docs/ios-signing.md`).

Pipeline **[stated in docs]**: host `cargo build` → `uniffi-bindgen` → `Generated/*`; iOS `aarch64-apple-ios` staticlib to `build/ios-device/lib/`; SwiftPM + `xtool dev` (`docs/architecture.md`, `docs/toolchain-validation.md`).

**New app** **[stated in docs]**: `forge init <Name> --bundle-id …` creates `examples/<Name>/` inside the **workspace** (not a standalone repo in v0.1) (`docs/project-generation.md`, `README.md`).

---

## 3. State and persistence across FFI

### Authority **[stated in docs]**

- Authoritative counter state lives in Rust (`forge-core` inside the app object) (`docs/architecture.md`).
- Actions commit **only if save succeeds**: candidate apply → persist → commit engine; failed save leaves memory and disk at last committed state (`docs/architecture.md`).

### Startup **[stated in docs]**

| Condition | Expected behavior |
|-----------|-------------------|
| No snapshot | Fresh install |
| Valid snapshot | Restore |
| Corrupt / unsupported | Error, no silent reset |
| Read failure | Error, no silent reset |
| Missing storage directory | Error |

Explicit recovery path for corruption **[stated in docs]** (architecture names `ForgeCounterApp.recoverFromCorruption`; ffi-policy names optional `recoverFromCorruption` in Swift).

### What crosses FFI **[stated in docs]**

- Records/enums/errors (`ForgeAppError`), action kinds, snapshots, path strings.
- Not: raw `CounterEngine`, internal `StorageError`, or unbounded nested structures (`docs/ffi-policy.md`).

---

## 4. What a new app author touches

### Should touch **[stated in docs + inference]**

- `forge init` output: `forge.toml`, `xtool.yml`, `Package.swift`, `Sources/AppHost/` SwiftUI (sample Counter UI in template) (`docs/project-generation.md`).
- Placeholders replaced: package name, bundle id, app support dir name (`docs/project-generation.md`).
- Workspace-root Rust changes only if extending **shared** `forge-core` / `forge-ffi` for new domain features **[my inference]** from “Rust crates always built from workspace root” (`docs/project-generation.md`).

### Should not touch **[stated in docs]**

- Generated UniFFI files under `Generated/` (regenerate via script/CLI) (`docs/ffi-policy.md`).
- Apple credentials in `forge.toml` (`docs/ios-signing.md`).
- Counter/domain rules duplicated in Swift (`docs/architecture.md`).

### v0.1 limitation **[stated in docs]**

- Generated apps are **workspace-local**; static library path is relative to repo (`../../build/ios-device/lib`) (`docs/project-generation.md`, ADR 001).

---

## 5. Error handling, versioning, and `doctor`

### Error handling **[stated in docs]**

- Domain vs persistence vs corruption vs internal categories with distinct FFI errors (`docs/ffi-policy.md`).
- Expected failures as `Result`, not panics across boundary; lock poisoning mapped to internal failure (`docs/ffi-policy.md`).
- Swift: inline messages for domain errors; blocked startup for corruption/read/directory failures with retry/recovery where documented (`docs/ffi-policy.md`, `docs/architecture.md`).

### Versioning **[stated in docs]**

- JSON snapshot **schema version** in envelope; filename includes version suffix (`forge-counter-v1.json`) (`docs/architecture.md` — storage section).
- UniFFI/workspace dependency pin in root `Cargo.toml` (`docs/ffi-policy.md`).
- **Migration** of old schema: not described as implemented **[my inference: gap unless code adds migrations]**.

### Toolchain validation / `doctor` **[stated in docs]**

- Reports Rust, Swift, xtool, targets, bindgen availability, SDK presence, device/auth blockers (`docs/toolchain-validation.md`, `scripts/validate-toolchain.sh` referenced).
- `forge doctor` delegates to validation script when present (`docs/architecture.md` implies wrapper).
- CI on hosted Linux: fmt, clippy, tests, binding drift — **not** full iOS link/device (`README.md` Known limitations, `docs/linux-setup.md`).

### Honesty constraints **[stated in docs]**

- `docs/toolchain-validation.md` records **verified** runs vs external prerequisites (`docs/toolchain-validation.md` preamble).
- Device-first; simulator off by default in config (`README.md`).

---

## 6. Ideal properties (inferred product bar)

These are **[my inference]** from “integration reference” and Linux-first goals:

- **Single orchestration path**: CLI invokes the same scripts developers can run manually (no divergent logic).
- **Template parity**: `templates/ios-app` matches what `examples/counter` demonstrates after init substitutions.
- **Per-app bindgen destination**: init generates bindings into the new app’s `Generated/`, not only the Counter example path.
- **Docs and troubleshooting stay consistent** with persistence contract (e.g. no “save issue” text that contradicts commit-on-save policy).

---

*Phase 2 audit compares this document to the implementation in `crates/`, `examples/counter`, `templates/ios-app`, `scripts/`, `Makefile`, and `.github/workflows/`.*
