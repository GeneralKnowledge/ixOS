# Where ixOS reinvents existing tools (2026-11-11)

The question: has ixOS rebuilt things that already exist? Each major component was compared with the closest existing tools, and given a verdict: justified, partly (adopt or wrap something for part of it), or not justified. The decisions that came out of it are in the [roadmap](../roadmap.md#tooling-freeze-and-reuse-rules), [architecture.md](../architecture.md#framework-principles) (native first) and the root `AGENTS.md` (rule 11). Reuben agreed with the assessment on 2026-11-11.

**Snapshot.** Every number here was measured at `main` c113907 on 2026-11-11 and is not kept up to date. Re-measure before relying on one. Line counts are non-test Rust or Swift at that commit (code before the first `#[cfg(test)]` in each file). They are a proxy for what each part costs to own, not a measure of its value.

## Summary

Mostly no. The framework crates (`forge-app`, `forge-runtime`, `forge-storage`, `forge-net`, `forge-services`) are about 7,700 of roughly 38,000 lines. They already stand on UniFFI, rusqlite, `idevice`, xtool, Appium's prebuilt WebDriverAgent, `rmcp`, `addr2line` and `object`, and wrap Apple's frameworks rather than replacing them. The other ~30,000 lines are tooling, and that is where the reinvention is.

## Ranked by custom code that could go

"Could go" means what adopting the alternative, or freezing, would remove or stop growing.

| # | Component | Custom lines | Closest existing alternatives | Verdict | Could go | Cost to switch |
|---|---|---|---|---|---|---|
| 1 | Exploration, focus, run records, MCP (`forge ui explore`, `forge ui focus`, `forge verify --run`, `forge mcp`) | ~8,100 | Maestro, Appium, Firebase Robo, mobile-mcp | Justified technically, over-built for current priorities | Freeze ~8,100 | None to freeze |
| 2 | Evidence bundles, reports, verify, history | ~2,300 | JUnit XML, Allure, Xcode's xcresult | Partly | ~500 (report rendering) | ~2 days to add JUnit XML |
| 3 | iOS HTTP transport (`UreqClient` with Rustls on iOS) | ~1,450 crate, ~400 transport | URLSession | Not justified on iOS (already decided) | The iOS transport path | Roadmap item, with background transfers |
| 4 | Native services (`forge-services` and Swift adapters) | ~2,700 Rust, ~2,100 Swift | CoreLocation, CoreMotion, CoreBluetooth, NWPathMonitor called from Swift | Partly | Services the Rust core never reads | Low: a rule for new services |
| 5 | Live dev connection (`forge-devproto`, `forge-devtools`, `forge device connect`) | ~1,800 | os_log through the syslog relay, WebDriverAgent, Pulse | Partly | Freeze ~1,800 | Nothing to adopt |
| 6 | Crash symbolication (`forge-crash`) | ~1,500 | Sentry's `symbolic` crates, `atos` (Mac only) | Justified | ~300 to 500 | ~3 days, not worth it now |
| 7 | `forge.toml` app config | ~1,300 | xtool.yml, XcodeGen, Tuist | Justified | 0 | n/a |
| 8 | Visual regression | ~1,200 | swift-snapshot-testing, pixelmatch, odiff | Justified | ~150 (pixel diff) | Not worth it |
| 9 | `forge setup`, `forge doctor` | ~1,300 | `xtool setup`, `xtool sdk`, `xtool auth` | Justified (already calls xtool) | 0 | n/a |
| 10 | Storage (`Document`, `FileStore`, `Database`) | ~1,200 | rusqlite_migration, GRDB, SwiftData, Core Data | Justified | ~60 (migration runner) | Hours, optional |
| 11 | Runtime (events, background, lifecycle, logging) | ~1,200 | tokio channels, `oslog` crate, tracing | Justified; logging partly | Logging via os_log | ~1 day, optional |
| 12 | App core (`forge-app` runtime and command executor) | ~1,200 | Crux (`crux_core`) | Justified | 0 | Weeks: every app's core and Swift layer |
| 13 | ForgeUI and ForgeAppKit | ~1,400 and ~650 Swift | SwiftUI styles, swift-navigation, TCA | Justified while small | 0 | n/a |
| 14 | Capability ledger generator | ~500 | Hand-edited Markdown | Justified (stops merge conflicts) | 0 | n/a |
| 15 | UniFFI boundary, ForgeKit host, device access, WDA, build and sign | ~1,200 glue | UniFFI, `idevice`, Appium WDA, xtool (already used) | Already adopted | 0 | n/a |

## Why

**Exploration, run records and MCP.** Maestro does not support physical iPhones, and Appium's non-macOS support needs iOS 18 or later and a root tunnel ([forge-ui-testing.md](../forge-ui-testing.md), [agent capability audit](forge-agent-capability-audit.md)). So this does not duplicate a tool that works here. The problem is scale: it is agent tooling, which the 2026-11-09 direction put last (P6). The MCP server already uses the official `rmcp` SDK and runs `forge` commands, which is the right shape.

**Evidence and reports.** The evidence bundle records requirement links and file hashes, which JUnit XML can't, so it stays. But it is a format nobody else reads, so a JUnit file beside it lets any CI system or test viewer show the results.

**iOS networking.** On iOS, Rust opens its own TLS connections. URLSession brings system proxy and VPN behaviour, HTTP/3, Low Data Mode and background transfers. This departs from native first, and the [native capability roadmap](../native-capability-roadmap.md) already plans the move. `forge-net`'s request types, `Backoff` and cancellation are portable and stay; ureq stays for Linux tools, tests and Android.

**Native services.** A Rust trait, a Swift adapter and generated glue pay off when the Rust core reads the value (connectivity and battery feed download rules). When only the screen shows a value, as with motion and Bluetooth in Device Lab, the Rust layer is a detour around Apple's frameworks.

**Live dev connection.** A Noise handshake, pairing and CBOR messages. Nothing off the shelf fits (Flipper is retired; Pulse is a Swift network logger), but owning protocol and cryptography code is a cost. It works, so it is frozen rather than removed.

**App core and Crux.** Crux is the closest project (a Rust core, a Swift shell, UniFFI), but it is an Elm-style architecture where every side effect is a request the shell fulfils. The `forge-app` executor is small and adds lifecycle, cancellation and persistence rules that Crux leaves to the shell. Switching means rewriting every app for no new capability. One idea is worth borrowing for the planned `forge_app::testing` harness: Crux tests effects as data.

**Storage.** GRDB, SwiftData and Core Data are Swift-side; Rust owns the state here, so rusqlite (bundled) is the right base. `FileStore` has no equivalent crate.

**Crash symbolication.** Already built on `addr2line`, `object` and `rustc-demangle`, with `swift demangle` from the toolchain. Sentry's `symbolic` could replace parts; whether it reads the newer `.ips` JSON reports was not checked.

**Others.** `forge setup` runs `xtool sdk install` and `xtool auth login` rather than duplicating them. XcodeGen and Tuist generate Xcode projects, which xtool doesn't use. swift-snapshot-testing needs a simulator, so a Mac; comparing real-device screenshots on Linux with the accessibility tree is not offered elsewhere. ForgeUI styles real SwiftUI controls; it is justified while it does not grow its own navigation or state framework.

## Decisions

1. Freeze the agent and device tooling (rows 1 and 5) at bug fixes until the roadmap's P1 to P4 exit criteria are met, with a review on 2026-12-15.
2. Move iOS networking to URLSession, as the native capability roadmap says.
3. A new native service gets a Rust interface only when the Rust core reads its value.
4. Write JUnit XML beside the evidence bundle; no new report features.

Optional, when the code is next touched: os_log instead of `syslog(3)` for iOS logging, and `rusqlite_migration` for the migration runner.
