# Chat acceptance report

What has been checked, where, and what is still to do, per milestone. Results are kept apart by kind: **automated** (CI on every PR), **local build** (by hand on a development machine) and **physical device** (an iPhone, run by Reuben). A device step is marked done only when it was actually performed and reported. Simulator screenshots are review aids, never acceptance results.

Status as of 2026-11-07.

## M0: Chat on the current framework

### Automated and local

| # | Check | Kind | Status | Evidence |
|---|-------|------|--------|----------|
| A1 | chat-core: accounts, tokens, `Unauthorized`, pause/resume streams | automated | Done | `cargo test -p chat-core` (62 tests) |
| A2 | chat-server: accounts, the server owns the author, data survives a restart, owner-only data file, corrupt or newer data refused | automated | Done | `cargo test -p chat-server` (unit + end-to-end over sockets) |
| A3 | chat-cli against a real server: signup, whoami, rooms, send, read, follow | local build (Linux) | Done | Run by hand in the cloud dev environment, 2026-11-07 |
| A4 | The app's Rust crate (`ChatCore`): sign-up keeps the token in the Keychain adapter and in no file, relaunch is signed in offline, two accounts with the same name, background/resume catch-up, a server that forgot the account, sign-out | automated | Done | `cargo test --manifest-path examples/chat/rust/Cargo.toml` (7 tests against the real server in process); `scripts/check-app-crates.sh` in CI |
| A5 | SwiftUI host compiles; model tests against the real Rust library and the real server | automated (macOS CI) | Done | `scripts/test-swift-host.sh examples/chat`; macOS CI green on main at be8d70e |
| A6 | Unsigned `xcodebuild` of the iOS app | automated (macOS CI) | Done | macOS workflow, "Build Chat.xcodeproj"; green on main at be8d70e |
| A7 | Frozen Android client against the new server (anonymous mode) | automated (CI, JVM) | Done | Android workflow, Chat JVM unit tests; green on main at be8d70e |

### On an iPhone (Linux, xtool)

Before you start: the laptop and the iPhone on the same Wi-Fi; the iPhone on USB and unlocked. This is also the first time the `forge-services` Keychain adapter runs on a device (the native services check app has not been run yet), so D4, D9 and D12 are its first real test.

| # | Step | Expected | Status |
|---|------|----------|--------|
| D1 | `git pull`, then `make chat-server ARGS="--bind 0.0.0.0:8787"` | "chat server on http://0.0.0.0:8787 (data in …/server.json; 0 accounts…" | Done. "chat server on http://0.0.0.0:8787 (data in /home/red/.local/share/ixos-chat/server.json; 0 accounts…". First attempt failed: an old `mock-chat-server` from 2026-11-05 still held port 8787 ("Address already in use"); stopped it, then it started |
| D2 | `forge run ios --project examples/chat` (debug) | Builds and installs; note the build time | Done. Installed with no xtool prompts; Swift link step 12.9 s (first build also compiled the Rust dependencies, several minutes). 26 `ld64.lld: warning: This version of lld does not support linking for platform iOS` lines, harmless |
| D3 | First launch | Sign-up screen; iOS asks for local network access, then notifications | Done (reported "working as intended") |
| D4 | Enter your name and `http://<laptop IP>:8787`, tap **Create Account** | The Chats list with General, Random, Forge dev | Done |
| D5 | `make chat-cli ARGS='signup "Laptop"'`, then `make chat-cli ARGS="send general Hello phone"` while General is open on the phone | The message appears within a second, grey, labelled "Laptop" | Done |
| D6 | Send from the phone in General; `make chat-cli ARGS="follow general"` on the laptop | The bubble goes Sending → Sent (blue, on the right); the laptop prints it with your name | Done |
| D7 | Go back to the list; `make chat-cli ARGS="send random ping"` | A banner "Random / Laptop / ping"; Random's badge shows 1 | Done (reported "working as intended") |
| D8 | Press Home for 30 s; send two messages from the laptop to General; open the app again | General catches up on both (open it); no duplicate bubbles | Done |
| D9 | Force-quit the app (app switcher), turn on Airplane Mode, open the app | Signed in at once (no sign-up screen); stored messages visible; a "can't reach" note on the list | Done |
| D10 | Airplane Mode off; pull to refresh | Rooms load; sending works | Done |
| D11 | Stop the server (Ctrl-C) with General open, send a message, restart the server, tap the red bubble | "Reconnecting…", the bubble turns red, then Sent after the tap; the earlier messages are still there (the server kept them on disk) | Done. The server ran in the background, so it was stopped with `pkill` and restarted on the same data file instead of Ctrl-C |
| D12 | Delete the app and reinstall it (`forge run ios` again) | iOS normally keeps Keychain items across a reinstall, so expect to be signed in again without sign-up and the chats to reload from the server. If it shows sign-up instead, note it: that tells us how xtool's signing treats the Keychain | Done. Signed in again after the reinstall and the chats came back: the Keychain item survives xtool re-signing |
| D13 | Stop the server, move `~/.local/share/ixos-chat/server.json` away, start it again, pull to refresh in the app | "The server no longer accepts this account" on the list; Settings → Sign Out → the sign-up screen; signing up again works | Done. Data file moved to `server.json.bak` (kept) |
| D14 | `forge run ios --project examples/chat --release` | Builds, installs and runs like D4–D6 | Done. Release build in 18.3 s, installed with no prompts; still signed in, received "Hello release" from the laptop, and the laptop's `follow` printed the phone's reply live |

Record for each run: iPhone model, iOS version, xtool version, commit, and anything that differed.

**Run 1, 2026-11-07 (10:28–10:54 UTC):** all of D1–D14 passed, reported by Reuben. iPhone "Cati" (UDID 00008130-…; model and iOS version not recorded), Linux laptop with xtool 1.24.0-ixos.1 (patched), commit be8d70e on main. The laptop side (server, chat-cli, installs) was run through a Claude Code session on the laptop; every on-phone result is Reuben's report.

## Framework problems found and fixed in M0

| Problem | Layer | Fix | Regression test |
|---------|-------|-----|-----------------|
| An app crate that depends on a crate outside Forge's own (Chat's `chat-core`) was not rebuilt when that crate changed, so a stale library could reach the phone | build scripts | `scripts/forge-crate.sh` adds every `path =` dependency in the app's `Cargo.toml` to the directories it watches | Checked by hand (`SCAN_DIRS` now lists `examples/chat-core`); no automated test yet |
| `ForgeServices.install()` makes Forge the notification delegate; the old Chat notifier replaced it, which would break notification taps and deep links | app | The notifier no longer sets a delegate; Forge's delegate shows the banners | None automated (needs a device: D7) |

## Gaps this milestone exposed (for later milestones)

- **No app-level way to watch a non-Forge path dependency** beyond direct `path =` lines (dependencies of dependencies are not scanned).
- **Notifications still go through Swift**, not `forge-services`' `LocalNotifier`; M5 moves them.
- **A stale server on the port is hard to spot.** chat-server only says "Address already in use"; it could name the process holding the port, or `forge doctor` could check it.
- **The server is plain HTTP**, so the token crosses the LAN in the clear. Fine on a home network; anything else needs a TLS proxy.
