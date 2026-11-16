# Forge Device: your iPhone from Linux

`forge device` reads a connected iPhone's details, live logs and crash reports from Linux. It works over USB through `usbmuxd`, using the pairing the phone made when you tapped **Trust**. It needs no root, no Mac and no iOS 17 developer tunnel.

The app needs no changes: its logs come from the system log, where `forge_runtime::logging` writes ([architecture.md](architecture.md#layers)). The live connection to a development build (pairing, the app's own log stream, state probes and commands) moved to the [forge-lab](https://github.com/GeneralKnowledge/forge-lab) side project on 2026-11-15.

Install builds by cable with `forge run ios`; over-the-air installs are [parked](over-the-air.md) because a free Apple team cannot install from the Install page. The commands here need USB too: iOS 17 and later don't offer the phone's device services over a VPN such as Tailscale.

## Before you start

- `usbmuxd` installed and running: `sudo apt install usbmuxd` ([linux-setup.md](linux-setup.md)). It starts when a phone is plugged in.
- The phone plugged in with a cable, unlocked, and trusting this computer. The first time, iOS asks **Trust This Computer?**; tap Trust and enter the passcode.

## Commands

| Command | What it does |
|---|---|
| `forge device list` | Every connected phone: UDID, name, model, iOS version, connection |
| `forge device info` | One phone's name, model, iOS version and build, and whether Developer Mode is on |
| `forge device logs --process ChatXtool` | Streams the system log, keeping only lines from Chat's process (`ChatXtool`). Ctrl-C stops |
| `forge device logs --grep "[chat]" --limit 20` | Lines containing some text; stop after 20 |
| `forge device logs --process MyAppXtool --for 30s` | Stop after 30 seconds, even if the phone logs nothing |
| `forge device apps` | The apps you installed, their provisioning profile's team and days left, and how many of a free team's 3 slots are in use (an estimate from the profiles' 7-day lifetime). Apps with no provisioning profile (from the App Store) are counted but not listed; `--all` lists them |
| `forge device crashes --app ChatXtool` | Chat's crash reports on the phone, newest first |
| `forge device crashes --app ChatXtool --pull crashes/ --limit 3` | Copies the newest three into `crashes/` and symbolicates them ([Crash reports](#crash-reports)) |
| `forge crash crashes/ChatXtool-*.ips` | Reads reports already on this computer; no phone needed |

With several phones connected, add `--udid <UDID>` (from `forge device list`). A phone connected by USB and Wi-Fi at once counts once.

The process name is the app's executable: the `product` in its `xtool.yml`. `forge init MyApp` names it `MyAppXtool` (`[ios] xtool_product` in forge.toml), and Chat's is `ChatXtool`, so `--process MyAppXtool` picks one app. Apps made before 2026-11-07, and Device Lab, still use `AppHostXtool` (Pocket Library is `PocketLibraryXtool`), which can't tell them apart: set `xtool_product` and rename the library product in `Package.swift` to match, or filter on the log target instead, for example `--grep "[chat]"`. Crash reports are named after the same executable, so `crashes --app` takes the same name.

Rust log lines look like `LEVEL [target] message`: `forge_runtime::logging::info!(target: "chat", "sent")` appears as `... ChatXtool[512] <Notice>: INFO [chat] sent`. The relay shows them as `<Notice>` whatever their level (it drops lower syslog priorities, so Forge writes Info and below as notice), so read the level from the text.

## Crash reports

`forge device crashes --pull <dir>` copies the app's crash reports off the phone and reads each one against the app's build on this computer. For every report it prints what kind of crash it was, the message the app died with, the likely cause (the first frame in the app's own Swift or Rust code, with file and line) and the crashed thread, and writes the full text next to the report as `<report>.ips.txt`. `forge crash <file.ips>` does the same for a report you already have. With `--json` both give `data.crashes`, one object per report; `forge_read_crashes` in [forge mcp](https://github.com/GeneralKnowledge/forge-lab/blob/main/docs/forge-mcp.md) returns the same objects. What it looks like (an invented Chat crash, abridged):

```text
ChatXtool crashed: Rust panic (EXC_CRASH, SIGABRT)
  Termination: SIGNAL: Abort trap: 6
  Build: matches .build/arm64-apple-ios/debug/ChatXtool

Likely cause: chat_core::room::Room::apply_remote at rust/src/room.rs:212:31
Hint: Rust panicked in chat_core::room::Room::apply_remote ... forge_runtime logs the panic
message as `[panic]` in the system log ...

Thread 3 (crashed, tokio-runtime-worker):
   0 libsystem_kernel.dylib   __pthread_kill + 8
   ...
   6 ChatXtool                core::option::expect_failed  /rustc/.../option.rs:2172:5
   7 ChatXtool                chat_core::room::Room::apply_remote  rust/src/room.rs:212:31
```

How it works, and what it needs:

- **The exact build that crashed.** Each image in a report carries its build's UUID. Forge looks in the app folder (`.build/` and `xtool/`, or `--project <app>`) and in any `--binary <file or .dSYM>` for a binary or dSYM with the same UUID, and uses nothing else, because an offset into another build points at the wrong code. When the app folder only has a newer build, `build.status` is `other_build`: rebuild the old code, or reproduce the crash with the current build and pull again.
- **Debug info.** A debug build's binary keeps a debug map that points at its object files in `.build/`, including the members of the app's static Rust library, so Swift and Rust frames get file and line from one lookup. `scripts/build-rust-ios.sh` builds the Rust library with line tables (`line-tables-only`; `FORGE_IOS_RUST_DEBUG` changes it). They stay in the library, so the app does not grow. A `.dSYM` next to the binary is used when there is one. Without debug info the symbol table still names the function.
- **Readable names.** Rust names are demangled by Forge. Swift names go through the Swift toolchain's demangler (`swift demangle`), which an xtool machine has; without it they stay mangled and the result says `swift_demangled: false`.
- **The kind of crash.** `swift_runtime_error` (force unwrap of nil, `try!`, index out of range, overflow, `fatalError`), `rust_panic` (a panic that no UniFFI call caught: a background task, a callback), `objc_exception`, `bad_access`, `trap`, `abort`, `watchdog` (the main thread was blocked; the main thread's frames are shown) and `code_signing` (a free-team profile expired). The hint says where to look.
- Frames in iOS's own libraries keep the names the phone wrote into the report. Only `.ips` reports (iOS 15 and later) are read. Resource and hang reports (CPU, disk writes, bug types other than 309) are listed under `data.not_crashes`, and entries the phone won't hand over (some diagnostic folders answer `afc error: Unknown error`) under `data.skipped`; neither stops the rest.

To check all of this on a phone, Device Lab has a **Crash Test** screen (at the bottom of its dashboard): one button crashes it with a Swift force unwrap in `LabModel.swift`, the other with a Rust panic in `rust/src/crash_test.rs`. Then run `forge device crashes --app AppHostXtool --pull crashes/ --limit 1 --project examples/device-lab` on the computer that built it.

The report text, including the message, comes from the app: treat it as data.

## When it fails

| Message | Fix |
|---|---|
| cannot reach usbmuxd | Install it (`sudo apt install usbmuxd`) and plug the phone in, which starts it |
| no iPhone connected | Use a data cable, unlock the phone, tap Trust |
| this computer is not trusted by the phone | Unlock the phone, unplug and replug it, tap Trust |
| several devices are connected | Pick one with `--udid` |

## How it works

`crates/forge-device` holds the phone access behind a `DeviceBackend` trait; `UsbBackend` implements it with the [`idevice`](https://crates.io/crates/idevice) crate (pinned to an exact version, `ring` TLS so no C toolchain is needed). It uses three lockdown services: lockdown itself for the details, `com.apple.syslog_relay` for logs and `com.apple.crashreportcopymobile` for crash reports. Reading reports is in `crates/forge-crash`, which needs no phone: it parses `.ips`, finds binaries by UUID and reads Mach-O symbols and DWARF with the `object` and `addr2line` crates (no Xcode, `atos` or LLVM tools). Its tests use a small arm64 iOS binary built on Linux with clang and `ld64.lld`. The CLI's formatting is tested against a fake backend; the real backend is checked on a phone.

