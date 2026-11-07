# FORGE_APP_NAME

> **Work in progress.** Forge is focusing on iOS for now; Android keeps building but gets no new features. See https://github.com/GeneralKnowledge/ixOS/blob/main/docs/roadmap.md#android--work-in-progress

An Android app made with [Forge](https://github.com/GeneralKnowledge/ixOS): the logic is Rust, the screen is Jetpack Compose.

## Run it

From this directory:

```bash
forge doctor                    # is the toolchain ready? (JDK, Android SDK and NDK, cargo-ndk)
forge build android --install   # build a debug APK and install it on a connected phone or emulator
forge test                      # Rust tests and the JVM unit tests
```

## Where things are

| Path | What it is |
|------|------------|
| `rust/src/lib.rs` | Your app's logic, state, persistence and networking. Everything marked `#[uniffi::export]` is callable from Kotlin. |
| `rust/Cargo.toml` | Rust dependencies (`cargo add --manifest-path rust/Cargo.toml <crate>`). |
| `app/src/main/java/` | The Compose screen and the view model that calls Rust. |
| `app/src/test/` | JVM unit tests against the real Rust library. |
| `Generated/` | Kotlin bindings generated from `rust/`. Do not edit; `forge build android` and `forge bindings generate` rewrite them. |
| `forge.toml` | App name, application id, minimum SDK and ABIs. |

## The loop

1. Change the Rust in `rust/src/lib.rs`; `cargo test --manifest-path rust/Cargo.toml` tests it in seconds.
2. If you changed what is exported, `forge bindings generate` updates the Kotlin API.
3. Use it from the Compose code, then `forge build android --install`.

`forge doctor` lists anything missing with the command that fixes it; the Forge checkout's `docs/android-setup.md` and `docs/troubleshooting.md` have more.
