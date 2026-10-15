# Toolchain validation

This document records **verified** toolchain steps on Linux for Forge Mobile. It is updated from real runs, not assumed capabilities.

## Environment snapshot (2026-10-15, Cloud Agent VM)

| Component | Version / status |
|-----------|------------------|
| OS | Ubuntu 24.04.5 LTS x86_64 |
| Rust | stable 1.99.0 (via `rust-toolchain.toml`) |
| Rust target `aarch64-apple-ios` | Installed via `rustup target add` |
| Swift | 6.2.3 (swift.org tarball for ubuntu24.04) |
| xtool | 1.22.2 AppImage at `/usr/local/bin/xtool` |
| Darwin Swift SDK | **Not installed** — requires `xtool setup` + local `Xcode.xip` |
| usbmuxd | Not verified in CI image |
| Physical iPhone | **Not available** in Cloud Agent VM |

## Verified commands (Linux)

### Rust workspace (no Apple SDK)

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

**Outcome:** Pass (14 unit/property tests at time of writing).

### UniFFI binding generation (host library)

```bash
./scripts/generate-bindings.sh examples/counter/Generated
```

**Outcome:** Produces `forge_ffi.swift`, `forge_ffiFFI.h`, `forge_ffiFFI.modulemap`.

Uses:

```bash
cargo build -p forge-ffi
cargo run -p uniffi-bindgen -- generate \
  --library target/debug/libforge_ffi.so \
  --language swift \
  --out-dir examples/counter/Generated
```

### iOS Rust static library

```bash
./scripts/build-rust-ios.sh
```

**Outcome (without Darwin SDK):** Fails with:

```text
[forge] warning: Darwin Swift SDK not installed (run: xtool setup)
[forge] error: SDKROOT is not set. Install the Darwin SDK via xtool setup and Xcode.xip.
```

This is an **external prerequisite**, not a Rust compilation bug on Linux.

### Full packaging script

```bash
./scripts/package-ios-library.sh
```

Runs binding generation, then attempts iOS static library build (same SDK requirement).

### Toolchain report

```bash
./scripts/validate-toolchain.sh
```

Runs non-interactive checks. `xtool devices` is wrapped in `timeout 5` to avoid hangs when auth/devices are unavailable.

## Integration strategy (confirmed)

1. **Host** `cdylib` for UniFFI Swift generation (Linux CI).
2. **`aarch64-apple-ios` `staticlib`** staged to `build/ios-device/lib/libforge_ffi.a` when `SDKROOT` is set from xtool’s Darwin SDK.
3. **SwiftPM** links the static library via `ForgeFfiC` target `linkerSettings` in [examples/counter/Package.swift](../examples/counter/Package.swift).
4. **xtool** builds the SwiftUI app: `cd examples/counter && xtool dev`.

**Fallback:** SwiftPM `.binaryTarget` artifact bundle (SE-0482) if `-L`/`-l` flags are rejected by a future SwiftPM version — not required for the current manifest.

**Rejected for v1:** `xcodebuild -create-xcframework` on Linux (tool not part of xtool Linux workflow).

## Manual acceptance: Counter on device

When Darwin SDK, signing, and a USB iPhone are available:

1. `xtool setup` (API key or password auth) and provide `Xcode.xip` path.
2. Confirm `swift sdk list` shows `darwin`.
3. From repo root: `cargo run -p forge-cli -- build ios --project examples/counter`
4. `xtool devices` — note device UDID.
5. After build, install/launch per `xtool` help for your version (typically via `xtool dev` with device attached).
6. Increment/decrement; force decrement at zero (error, no crash).
7. Kill app, relaunch — count persists under Application Support.
8. Clean rebuild without editing `Generated/*`.

**Cloud Agent status:** Steps 3–8 **not executed** here (no SDK, no device).

### xtool dev in restricted VMs

Running `xtool dev` without Darwin SDK may fail immediately. In Cloud Agent sandboxes, xtool 1.22.2 can also abort with `epoll_ctl: Operation not permitted` from SwiftNIO — an environment restriction, not an application bug. Use a full Linux workstation with USB, SDK, and normal epoll permissions for app builds.

## Counter checklist

| Test | Linux automated | Device manual |
|------|-----------------|---------------|
| Increment from zero | `cargo test -p forge-core` | UI |
| Decrement at zero rejected | yes | UI error message |
| Max 1_000_000 | yes | optional |
| Persistence save/load | `cargo test -p forge-storage` | relaunch |
| Corrupt JSON | yes | n/a |
| UniFFI bindings fresh | `./scripts/check-bindings.sh` | n/a |
