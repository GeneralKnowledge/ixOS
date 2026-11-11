# Forge Device: your iPhone from Linux

`forge device` reads a connected iPhone's details, live logs and crash reports from Linux, and talks to a development build of your app while it runs. It works over USB through `usbmuxd`, using the pairing the phone made when you tapped **Trust**. It needs no root, no Mac and no iOS 17 developer tunnel.

- **The phone** (M0): details, the system log and crash reports. The app needs no changes: its logs come from the system log, where `forge_runtime::logging` writes ([architecture.md](architecture.md#layers)).
- **The live connection** (M1): with a development build (`forge run ios --devtools`), pair once and then stream the app's own log and lifecycle, read its status and state probes, and run commands it registers. See [The live connection](#the-live-connection).

Wi-Fi, installing builds with verification, and over-the-air updates come later.

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

`forge device crashes --pull <dir>` copies the app's crash reports off the phone and reads each one against the app's build on this computer. For every report it prints what kind of crash it was, the message the app died with, the likely cause (the first frame in the app's own Swift or Rust code, with file and line) and the crashed thread, and writes the full text next to the report as `<report>.ips.txt`. `forge crash <file.ips>` does the same for a report you already have. With `--json` both give `data.crashes`, one object per report; `forge_read_crashes` in [forge mcp](forge-mcp.md) returns the same objects. What it looks like (an invented Chat crash, abridged):

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

## The live connection

A development build runs a small server inside the app (the `forge-devtools` crate). While the app is on screen it listens on the phone's loopback interface only; `forge device` reaches it through usbmuxd over the cable. It is compiled in only with `--devtools`, so ordinary and release builds contain none of it.

```sh
forge run ios --devtools          # build and install a development build; open the app
forge device pair                 # once per app and phone, with the app open
forge device connect              # the app's log and lifecycle, live; Ctrl-C stops
```

| Command | What it does |
|---|---|
| `forge device pair` | Pairs this computer with the app on screen. Run it once per app and phone |
| `forge device connect` | Streams the app's `forge_runtime::logging` records and lifecycle changes, starting with the last 50 (`--tail N`). When the app goes to the background it prints `[suspended ...]`, waits, and carries on from where it stopped when the app comes back, with nothing lost (`--no-follow` stops instead) |
| `forge device status` | App, build, phase, uptime, buffered records, probes and commands |
| `forge device snapshot` | The current value of every state probe the app registered |
| `forge device cmd` | Lists the app's own commands; `forge device cmd <name> [args]` runs one |
| `forge device unpair` | Forgets the pairing on the phone and on this computer |

Lines look like `+  12.345s INFO  [chat] sent`: the time since the app started, the level, the log target and the message.

### What an app exposes

The starter app (`forge init`) shows a `notes` probe and a `log` command (`forge device cmd log hello` writes "hello" to the app's log). In `rust/src/lib.rs`, the `devtools` module registers them; add your own there:

```rust
tools.probe("outbox", move || outbox.len().to_string());
tools.command("reconnect", "Drops and reopens the server stream", move |_args| {
    stream.reconnect();
    Ok("reconnecting".into())
});
```

A probe is a name and a function that returns text. A command gets string arguments and returns text or an error. That is all a host can reach: there is no shell and no way to run code the app did not register. Never expose secrets or user content (tokens, Keychain values, message bodies) through either.

### Pairing and security

- Pairing runs over USB. The app has no pairing yet, or it was started less than two minutes ago, so whoever pairs could also unlock the phone and relaunch the app. After that window, a paired app refuses new pairings.
- Every connection, USB included, is encrypted and authenticated with the Noise protocol (`Noise_XXpsk3_25519_ChaChaPoly_BLAKE2s`), keyed by a random 256-bit secret both ends stored when pairing. A host without the secret fails the handshake.
- The secret is kept in the app's own storage (`.forge-devtools-pairing` next to its data), so deleting the app forgets it, and on this computer under `~/.local/share/forge/devices/<udid>/<bundle-id>.key` (mode 0600; `$XDG_DATA_HOME` is honoured).
- The server listens on `127.0.0.1` only, and only while the app is in the foreground. Nothing listens on Wi-Fi.

### When the live connection fails

| Message | Fix |
|---|---|
| no development build is listening on the phone | Open the app so it is on screen, and check you installed it with `forge run ios --devtools` |
| ... is not paired with this computer | `forge device pair` with the app open |
| ... refused this computer's pairing key | The app was paired again or unpaired. Run `forge device pair` |
| ... is already paired with another computer | Close the app, open it, and run `forge device pair` within two minutes |
| the app speaks devtools protocol N and this forge speaks M | Rebuild and reinstall the app with this checkout: `forge run ios --devtools` |

## How it works

`crates/forge-device` holds the phone access behind a `DeviceBackend` trait; `UsbBackend` implements it with the [`idevice`](https://crates.io/crates/idevice) crate (pinned to an exact version, `ring` TLS so no C toolchain is needed). It uses three lockdown services: lockdown itself for the details, `com.apple.syslog_relay` for logs and `com.apple.crashreportcopymobile` for crash reports. Reading reports is in `crates/forge-crash`, which needs no phone: it parses `.ips`, finds binaries by UUID and reads Mach-O symbols and DWARF with the `object` and `addr2line` crates (no Xcode, `atos` or LLVM tools). Its tests use a small arm64 iOS binary built on Linux with clang and `ld64.lld`. The CLI's formatting is tested against a fake backend; the real backend is checked on a phone.

The live connection has three crates. `forge-devproto` is the wire protocol and does no I/O of its own: length-prefixed frames (65,535 bytes at most), a cleartext preamble that names the app and the protocol version (so a mismatch is explained in words), the Noise handshake, CBOR-encoded messages, and the numbered record buffer that lets a host reconnect without losing records. `forge-devtools` is the server inside the app: it copies log records and lifecycle changes into a buffer of 2,000 records, and answers one host at a time. `forge-device` connects to the app's port through usbmuxd and keeps the pairing secrets. Every test of the connection runs the real server against the real host client over loopback TCP, including pairing, wrong keys, the background and replay.
