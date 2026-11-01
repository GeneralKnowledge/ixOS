# Android setup (Linux)

Forge Mobile Android apps use **Rust `cdylib` + UniFFI Kotlin + Jetpack Compose**, built from the same `forge-ffi` crate as iOS.

## Prerequisites

| Tool | Purpose |
|------|---------|
| Rust stable | Workspace + `forge-ffi` |
| `rustup target add aarch64-linux-android x86_64-linux-android` | Device + emulator ABIs |
| Android SDK + NDK (r28+) | Gradle + 16 KB page-size defaults |
| `cargo install cargo-ndk` | Cross-compile `.so` into `jniLibs/` |
| JDK 17+ | Gradle |
| `adb` | Devices / install |

Environment variables (typical):

```bash
export ANDROID_HOME="$HOME/Android/Sdk"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/<version>"
export PATH="$ANDROID_HOME/platform-tools:$PATH"
```

## Validate

```bash
forge doctor
# or
./scripts/validate-toolchain.sh
./scripts/validate-android-toolchain.sh
```

## Build the reference app

From the repo root:

```bash
./scripts/package-android-library.sh examples/counter-android/Generated
cd examples/counter-android
./gradlew :app:assembleDebug
```

Or use the CLI:

```bash
cargo build -p forge-cli
forge build android --project examples/counter-android
```

Install on a running emulator or device:

```bash
forge build android --project examples/counter-android --install
```

## Emulator (Linux + KVM)

Create an AVD with an **x86_64** system image so the emulator uses the `x86_64` JNI library. Physical devices need **arm64-v8a** (included in default `forge.toml` ABIs).

## Storage

The Kotlin host passes an app-private directory string into `ForgeCounterApp` (typically under `Context.filesDir`). Domain persistence stays in Rust; do not hardcode paths in `forge-storage`.

## Troubleshooting

| Symptom | Likely cause |
|---------|----------------|
| App crashes on emulator, works on phone | Missing `x86_64` in `[android].abis` / jniLibs |
| `UnsatisfiedLinkError` | `libforge_ffi.so` not built or wrong ABI folder |
| Bindings compile errors | Regenerate: `forge bindings generate --language kotlin --out examples/counter-android/Generated` |
| Play 16 KB page warning | Use NDK r28+; run alignment probe in `validate-android-toolchain.sh` |

See also [toolchain-validation.md](toolchain-validation.md) and [ffi-policy.md](ffi-policy.md).

## forge.toml is the source of truth

`app/build.gradle.kts` reads `forge.toml` directly, so builds from Android Studio and from `forge build android` agree:

| forge.toml | Gradle |
|------------|--------|
| `[android] application_id` (or `[app] bundle_id` when unset) | `applicationId` |
| `[android] min_sdk` | `minSdk` |
| `[android] abis` | ABIs built into `jniLibs/` by `forge build android` |

The Kotlin package and Gradle `namespace` are fixed when `forge init` creates the app. Android needs `[build] android = true`; an `[android]` section alone does not enable it. Bundle IDs for Android apps must be valid Kotlin packages: no hyphens, and no Java or Kotlin keywords as segments.

Removing an ABI from `abis` deletes its staged library on the next build. For an unoptimized native build (useful for symbolicated crashes), set `FORGE_ANDROID_PROFILE=dev` (or `debug`); iOS uses `FORGE_IOS_PROFILE` the same way.
