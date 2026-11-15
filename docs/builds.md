# What each build produces

Forge has several ways to build an app. They differ in the host machine, the tool, where the app runs, and whether it is a debug or release build. This page lists them all, so it is clear which ones are supported and tested and which are not.

## At a glance

| Command or tool | Host | Platform tool | Runs on | Swift / Kotlin config | Rust profile | Status |
|---|---|---|---|---|---|---|
| `forge build ios` | Linux (or macOS) | xtool (`xtool dev build`) | iPhone (device) | debug | forge-dev | Supported. Counter, Weather and Chat installed on an iPhone (2026-11-06) |
| `forge run ios` | Linux (or macOS) | xtool (`xtool dev build`, then `xtool install` of that build once an iPhone is connected) | iPhone, installed (open it from the home screen; xtool does not launch apps) | debug | forge-dev | Supported, same runs as above |
| `forge build ios --release`, `forge run ios --release` | Linux (or macOS) | xtool (`xtool dev build -c release`) | iPhone (device) | release | release | Supported. Counter, Weather and Chat built in release with xtool on Linux (2026-11-06); not yet recorded on a device through this flag |
| `forge build ios --package-only`, then Xcode | macOS | Xcode (`xcodebuild`) | iPhone (device) | Debug or Release, chosen in Xcode | forge-dev (`FORGE_IOS_PROFILE=release` for a release build) | Supported. CI does unsigned Debug builds of the Chat, Pocket Library and freshly generated Xcode projects |
| `forge check swift` in an iOS app | Linux (or macOS) | Cargo, then plain SwiftPM (`swift test`), no iOS SDK | the host | debug | debug | Supported. Compiles the Swift files that import nothing Apple-only (view model, bindings) and runs the tests that need no SwiftUI; views are left out |
| `forge test` in an iOS app | Linux or macOS | Cargo, then SwiftPM (`swift test`) on macOS | the host | debug | debug | Supported, CI. The app's Rust tests run everywhere; the XCTest suite only on macOS (skipped with a note on Linux) |
| `forge build android` | Linux or macOS | Gradle (`assembleDebug`) | Android phone or emulator | debug APK | release | Supported. CI builds the APK; not yet run on a physical phone |
| `forge build android --install` | Linux or macOS | Gradle (`installDebug`) | connected phone or emulator | debug APK | release | Supported, not yet recorded on a physical phone |
| `forge test` in an Android app | Linux or macOS | Cargo, then Gradle (`testDebugUnitTest`) | the JVM (host tests) | debug | debug | Supported, CI |
| iOS Simulator | macOS | Xcode | Simulator | — | — | **Not supported**: no simulator slice of the Rust library ([roadmap](roadmap.md#ios-simulator-support--open)) |
| Release / store builds (App Store, Google Play) | — | — | — | — | — | **Not supported** yet. The staged plan for the first App Store release, and which steps need a Mac, is [first-app-store-release.md](first-app-store-release.md) |

Notes:

- **Device builds only on iOS.** The Rust library is built for `aarch64-apple-ios` alone ([ADR 001](adr/001-ios-static-linking.md)). A Linux machine cannot run the iOS Simulator anyway, so on Linux a physical iPhone is the only way to run an app.
- **The Rust library is optimised even in debug builds.** A debug Rust build is large and slow on a phone, so iOS development builds use the `forge-dev` profile (release optimisation without link-time optimisation, see [Build speed](#build-speed)) and `--release` builds Cargo's `release` profile. Android builds use `release`. Set `FORGE_IOS_PROFILE=debug` or `FORGE_ANDROID_PROFILE=debug` to build it unoptimised. Host tests (`forge test`, `cargo test`) always use the debug profile.
- **xtool builds use a debug Swift configuration** unless you pass `--release`, and are signed with your Apple ID for your own devices ([ios-signing.md](ios-signing.md)). They are development builds, not App Store builds. With Swift 6.2 they need the Darwin SDK from Xcode 26 ([troubleshooting.md](troubleshooting.md)).
- **Android builds are debug APKs**, signed with the SDK's debug key. A release build type, signing from the environment, an `.aab` bundle and R8 rules are on the roadmap.
- **Emulators.** The Android ABIs in `forge.toml` include `x86_64`, so the APK also runs on an x86_64 emulator. Android HTTPS has only been tested on the JVM and in Rust tests so far ([toolchain-validation.md](toolchain-validation.md#manual-acceptance-android-https-on-a-device)).

## Build speed

Where the time goes in an iOS build, and what keeps the next build short:

- **Rust, development builds: the `forge-dev` profile.** Every app crate's `[profile.release]` asks for whole-program link-time optimisation on one codegen unit (`lto = true`, `codegen-units = 1`): the smallest binary, but every Rust change re-optimises the whole program, Forge's crates and every dependency, on one core. Development builds (`forge build/run/publish ios` without `--release`) use `forge-dev` instead: `opt-level` 3 like release, but no LTO, 16 codegen units and incremental compilation. Measured on Device Lab's crate (which links every hosted app's core, 4 cores, 2026-11-15): clean 2 min 25 s instead of 4 min 29 s, and after an edit to one hosted app's core 11 s instead of about 2 min 10 s. The static library is larger (no LTO), and the linker's dead stripping drops what the app doesn't call. An app may define its own `[profile.forge-dev]` in its Cargo.toml; `--release` keeps the release profile.
- **Bindings come from the iOS library.** `forge build ios` used to build the crate a second time for the host, only so UniFFI could read the bindings from it (47 s more for Device Lab from clean). UniFFI reads the same metadata from the iOS static library, so packaging now builds the iOS library first and generates the bindings from it. `forge bindings generate` (no iOS SDK needed) still uses a host build.
- **Unchanged Swift files keep their timestamps.** Generating the bindings used to delete and rewrite `Generated/ForgeFFI/` on every build, and the fresh modification times made SwiftPM recompile the bindings module, and the modules that import it, even when nothing in Rust had changed. Now only files whose contents changed are written.
- **Each checkout and each app crate has its own Cargo target folder** (`<app>/rust/target`), so the first build of an app, or of the same app in another checkout (the `check-ios-xtool.sh --ref` worktree), compiles every dependency again. `CARGO_TARGET_DIR` (for example `~/.cache/forge/cargo-target`, set in the build host's shell profile) shares one folder between them; Cargo keeps builds with different dependency versions or features apart.
- **Swift.** The app's Swift builds in debug (`xtool dev build`), at most one compiler job per 2 GiB of the build's memory cap (see the queue below). A hosted app adds its whole Swift package to Device Lab's build, and a Device Lab build takes longer than a single app's. Phone testing still goes through Device Lab (decided 2026-11-15): over the air only updates an app already on the phone, and each app's first install needs the cable ([over-the-air.md](over-the-air.md#first-install-by-cable)), so one unified app keeps over-the-air updates working. Building one app on its own (`forge build ios --project examples/<app>`) is still the quick compile check. The limit that matters is 10 new App IDs a week, so reuse each app's bundle id.

## The build queue

A machine that several people or sessions build on (the laptop the apps are installed from) can't take several iOS builds at once: Cargo, SwiftPM and xtool each use every core and gigabytes of memory, and together they run it out of memory until it freezes (2026-11-14: `ps` hung and the laptop needed a forced restart). So every heavy build goes through one queue, and runs inside a cap:

- **One at a time, in arrival order.** `forge build ios`, `forge run ios` (its build part), `forge publish ios`, `scripts/check-ios-xtool.sh` and `forge queue run -- <command>` wait in line until it is their turn. A waiting build says its place in line and whose build is running:

  ```text
  [forge] Another build is running on this machine, so this one is queued: number 2 in line (`forge queue status` shows the queue; waits up to 60 min, FORGE_BUILD_LOCK_TIMEOUT). Building now: pid 4242 since 2026-11-14T15-10-02: forge publish ios in /home/red/ixOS/examples/device-lab (branch claude/ota-fix) [PR #189]
  ```

- **`forge queue status`** (`--json` too) shows the build running now and everyone waiting.
- **`forge queue run -- <command>`** puts any other heavy command in the queue: a raw `cargo build`, `swift build`, `xtool dev build`, or a script. It exits with the command's exit code. Use it for anything heavy that isn't one of the commands above.
- **Caps.** On Linux with systemd, each heavy step runs in a `systemd-run --user --scope` with `MemoryMax` (no swap) and `CPUQuota`, and Cargo and SwiftPM get fewer jobs: `CARGO_BUILD_JOBS` for Cargo, and for the Swift app build under xtool a `swift` first on PATH (`~/.cache/forge/swift-jobs-N/`) that adds `-j N` to `swift build` and passes everything else to the real one (SwiftPM ignores CPU pinning and the CPU quota, so without it one app build started over a hundred compilers on 2026-11-14). By default a build may use all but a quarter of the memory (at least 2 GiB stays free), every core but one, and one job per 2 GiB. It never takes memory that something already running needs: if another job (an ML model on the GPU, say) holds memory when the build starts, the cap shrinks to what is free less room for that job to grow (an eighth of the machine, at least 1 GiB). The build is then slower or may be stopped, and the other job is not touched. A capped build is the only thing the cap can stop: the kernel kills inside the build's own scope, never outside it. A build that outgrows its cap is killed by the kernel and fails with a message saying so, instead of freezing the machine. Builds also run at nice 10 and the lowest best-effort I/O priority, so Remote Control, SSH and the over-the-air install page come first.
- **Nothing stale.** `forge run ios` leaves the queue as soon as its build is done, so waiting for the phone or installing holds up no one. The lock (`flock` on `~/.cache/forge/build.lock`) frees itself when its process exits, however it exits, and places in line (`~/.cache/forge/build-queue/`) of processes that are gone are skipped. A build that hangs keeps its turn: waiters give up after an hour and name its pid, so you can stop it.

### Setting up a shared build host

Run this once on the machine (it asks for your sudo password), and again after any step it says is still to do:

```bash
cd ~/ixOS && git pull && scripts/setup-build-host.sh     # --check only reports
```

It turns on lingering for your user and delegates the memory controller to your user's systemd (so `systemd-run --user` caps work, also from Remote Control); installs `earlyoom`, which stops the biggest compiler first before the machine runs out of memory, even if something bypassed the queue, and never stops a protected process (below); installs the current `forge` into `~/.cargo/bin` and lists older copies that don't queue; and, once a test proves the cap stops a process at its limit, writes `~/.config/forge/build-host.toml`. With that file, forge refuses to build without the cap instead of risking the machine. The file can also set the caps (`memory_max_gib`, `cpu_quota_percent`, `jobs`).

### Protecting your own jobs

earlyoom never stops a protected process. The setup script protects:

- every process using a GPU when it runs (NVIDIA, AMD and Intel GPUs, and what `nvidia-smi` lists)
- common ML runtimes: `python`, `ollama`, `llama-server`, `llama-cli`, `jupyter-lab`, `torchrun`
- sshd, tailscaled, usbmuxd, Claude (`claude`, `node`) and systemd
- any name in `~/.config/forge/protected-processes`, one per line

To add a job, run `scripts/setup-build-host.sh --protect <process name>`. It is remembered for later runs. Run the script again after starting a new GPU job, so the job is picked up automatically. To protect a single running process from any out-of-memory kill, kernel or earlyoom, run `sudo choom -n -1000 -p <pid>`.

Elsewhere (a machine only you build on, macOS) the queue still works and a missing cap is only a note.

| Variable | Effect |
|---|---|
| `FORGE_BUILD_LOCK_NOTE` | Who is building (a thread, PR or person), shown to whoever waits (`forge queue run --note` too) |
| `FORGE_BUILD_LOCK_TIMEOUT` | Seconds to wait in line (default 3600) |
| `FORGE_BUILD_LOCK` | Another lock file, or `off` for no queue |
| `FORGE_BUILD_MEMORY_MAX` | The memory cap in GiB, or `off` for none |
| `FORGE_BUILD_JOBS` | Cargo jobs |
| `FORGE_BUILD_NICE=0` | Keep normal CPU and disk priority |

## Which one should I use?

- **On Linux with an iPhone:** use `forge run ios` from the app directory. Run `forge doctor` first: it lists anything missing for "iOS from Linux" and "Run on an iPhone" along with how to fix it. `forge run ios` repeats the quick checks before it builds.
- **On Linux with an Android phone:** use `forge build android --install`.
- **On a Mac:** both of the above work too. Use Xcode when you want the debugger or Instruments ([ios-xcode.md](ios-xcode.md)).
- **Changing the app's name, version, permissions or entitlements:** edit `forge.toml`; the next build writes Info.plist, App.entitlements and xtool.yml ([app-config.md](app-config.md)).
- **No device:** use `forge test` (the app's Rust tests, plus JVM tests on any host or XCTest on macOS). `forge test --workspace` also runs Forge's own tests.

Tested tool versions are in [compatibility.md](compatibility.md).
