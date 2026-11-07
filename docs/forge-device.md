# Forge Device: your iPhone from Linux

`forge device` reads a connected iPhone's details, live logs and crash reports from Linux. It talks to the phone over USB through `usbmuxd`, using the pairing the phone made when you tapped **Trust**. It needs no root, no Mac and no iOS 17 developer tunnel, and the app needs no changes: its logs come from the system log, where `forge_runtime::logging` writes ([architecture.md](architecture.md#layers)).

This is milestone M0 of Forge Device. Installing builds, a live connection to the app, and over-the-air updates come later.

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
| `forge device crashes --app ChatXtool` | Chat's crash reports on the phone, newest first |
| `forge device crashes --app ChatXtool --pull crashes/` | Copies them into `crashes/` |

With several phones connected, add `--udid <UDID>` (from `forge device list`). A phone connected by USB and Wi-Fi at once counts once.

The process name is the app's executable: the `product` in its `xtool.yml`. That is `AppHostXtool` for every app made with `forge init` (Chat's is `ChatXtool`), so `--process AppHostXtool` cannot tell two starter apps apart. Filter on the log target instead, for example `--grep "[chat]"`. Crash reports are named after the same executable, so `crashes --app` takes the same name.

Rust log lines look like `LEVEL [target] message`: `forge_runtime::logging::info!(target: "chat", "sent")` appears as `... ChatXtool[512] <Notice>: INFO [chat] sent`. The relay shows them as `<Notice>` whatever their level (it drops lower syslog priorities, so Forge writes Info and below as notice), so read the level from the text.

## When it fails

| Message | Fix |
|---|---|
| cannot reach usbmuxd | Install it (`sudo apt install usbmuxd`) and plug the phone in, which starts it |
| no iPhone connected | Use a data cable, unlock the phone, tap Trust |
| this computer is not trusted by the phone | Unlock the phone, unplug and replug it, tap Trust |
| several devices are connected | Pick one with `--udid` |

## How it works

`crates/forge-device` holds the phone access behind a `DeviceBackend` trait; `UsbBackend` implements it with the [`idevice`](https://crates.io/crates/idevice) crate (pinned to an exact version, `ring` TLS so no C toolchain is needed). It uses three lockdown services: lockdown itself for the details, `com.apple.syslog_relay` for logs and `com.apple.crashreportcopymobile` for crash reports. The CLI's formatting is tested against a fake backend; the real backend is checked on a phone.
