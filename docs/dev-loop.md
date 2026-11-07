# The Linux dev loop for native iOS apps: an assessment

Status: investigation, 2026-11-07. Nothing in this page has been built yet; it proposes small fixes, not a new layer.

The question (Reuben, 2026-11-07): is it easy to **create an app, change its UI, run it on a real iPhone, inspect what happened, diagnose problems and repeat**, from Linux, with Rust as the logic and SwiftUI as the presentation? This page walks that loop step by step with what Forge has on `main` today, measures what can be measured, ranks the gaps by friction and proposes the smallest fix for each.

Short answer: the **Rust half of the loop is fast and solid** (seconds, on Linux, with tests). The **Swift half and the phone half are slow and partly blind**. A Swift mistake surfaces either on the laptop after an xtool build, or, for anyone without the Darwin SDK (every cloud coding session), only after macOS CI, about 35 minutes later. After an install, someone has to tap the icon. Nobody can see the screen from the computer. Swift logs never reach the live stream. Most of the fixes are small and reuse code that already exists in the repo or in open PRs.

The same loop is what an AI agent would drive through a Forge MCP server (being designed separately, `docs/forge-mcp.md`). Each gap below says whether it **blocks an unattended agent**: a step that needs a human's hands or eyes on every iteration.

## The five areas at a glance

| Area | Where it stands | Biggest gap |
|---|---|---|
| Native UI (SwiftUI, navigation, controls, conventions) | Strong. [ios-ui.md](ios-ui.md) sets the standard; the starter, Pocket Library, Weather and Chat follow it | You cannot see it from the computer (no preview, no device screenshot) |
| Application logic (Rust, the Swift–Rust boundary) | Strong. App-owned crates, UniFFI, async bridge, cancellation, typed errors; Rust tests run in seconds | Boundary mistakes on the Swift side are found late (see gap 1) |
| Native capabilities | Growing. Keychain, notifications, permissions, deep links, files, sharing, audio, device services; see the [capability ledger](capability-ledger.md) | Most rows are still "iPhone: No". Proving one costs a full manual loop |
| Application infrastructure | Good. forge-storage (SQLite, files), forge-net, lifecycle, logging facade | Swift-side logs and errors don't reach the tools (gap 4) |
| **Development experience** | **The weakest area.** `forge init`, `forge run ios`, `forge doctor`, failure diagnosis and `forge device` exist and work. The loop between them has holes | Gaps 1–5 below |

So Reuben's suspicion holds: the capabilities are ahead of the loop that proves and uses them.

## Walking the loop

Times marked *measured* were measured for this page (cloud Linux container, x86_64) or copied from a recorded laptop run with its source. Everything else is a description, not a measurement.

| Step | What you do today | Time | Human needed every time? |
|---|---|---|---|
| 0. Toolchain | [linux-setup.md](linux-setup.md), `xtool setup` with Xcode 26's `Xcode.xip`, `forge doctor` | About an hour, once (getting-started.md) | Once |
| 1. Create | `forge init MyApp --bundle-id …` | *Measured* 1m29s cold (compiles the new crate to generate its bindings) | No |
| 2a. Change the logic | Edit `rust/src/lib.rs`, `cargo test --manifest-path rust/Cargo.toml` | *Measured* 3.4 s warm; `forge bindings generate` 0.7 s | No |
| 2b. Change the UI | Edit `Sources/AppHost/*.swift`. No `#Preview` on Linux (ios-ui.md) | No feedback at all until step 3 | n/a |
| 3. Build | `forge run ios`: preflight, Rust for iOS, bindings, `xtool dev run` | Laptop, recorded: clean 1m31s–2m11s; Chat's Swift link 12.9 s; Chat release 18.3 s. A no-change rebuild with stock xtool 13.1 s, with the patched xtool 2.9 s (xtool-upstream notes) | No, if the laptop has the SDK. **Cloud sessions have no Darwin SDK**, so they cannot do this step at all |
| 3'. Swift check without the SDK | Push and wait for macOS CI | *Measured* on main at fac9435: 22–29 min queued, then 9–10 min running (two jobs). A docs-only PR also takes these runners | No, but ~35 min per Swift mistake |
| 4. Install | Same command; xtool signs online, re-creates the profile, uploads the whole IPA | Not measured separately | First install: trust the profile on the phone. Every time the 3-app slot is full: delete an app on the phone |
| 5. Launch | **Tap the icon.** xtool can't launch on iOS 17+ (`forge run ios` says so) | n/a | **Yes, every iteration** |
| 6. Look at it | Look at the phone. Simulator screenshots exist only in macOS CI, for the starter, Pocket Library and Weather, and only when their Swift changes | n/a | **Yes, every iteration** |
| 7. Logs | `forge device logs --grep "[target]"` (system log, Rust records only); `forge device connect` with `--devtools` (Rust records and lifecycle, not yet run on the phone) | Live | No, but Swift `print`/`Logger` output is missing from the live stream, and every starter app's process is `AppHostXtool` |
| 8. Diagnose a crash | `forge device crashes --app … --pull` | Live | Reports are not symbolicated; no debugger from Linux |
| 9. Test | Rust tests on Linux. XCTest only on macOS (`forge test` skips it). UI tests on the phone: PR #61, draft, not yet run on the phone | Rust: seconds. XCTest: macOS CI (step 3') | UI checks are manual runbooks (iphone-acceptance.md, ACCEPTANCE.md) |
| 10. Repeat | Steps 2–7 again | | |

What the acceptance records show: Chat M0 (D1–D14) and M1 (E1–E13) passed on Cati, each with Reuben doing every on-phone step and a Claude session on the laptop running the commands. Chat M2 was skipped and M3 is unrun; Pocket Library's device steps (E1–E11, F2, F3) and the starter's (A3–A5, B1–B10) have never been run. The bottleneck is not the code: it is that every proof needs Reuben at the phone.

## Spike: Swift checks on Linux without the iOS SDK

The view model is where most Swift–Rust boundary mistakes live (a renamed method, a changed record field, an error case), and the starter keeps it free of SwiftUI: `NotesModel.swift` imports only Foundation and Observation. UniFFI's generated Swift is plain Foundation. So both should compile with the ordinary Linux Swift toolchain, against the app's Rust crate built for the Linux host.

What was done (cloud container, Docker `swift:6.2`, no Darwin SDK, a fresh `forge init` app):

1. `cargo build --manifest-path rust/Cargo.toml` gives `rust/target/debug/libforge_ffi.a` for Linux.
2. A throwaway SwiftPM package with three targets: `ForgeFFI` (only `Generated/ForgeFFI/forge_ffi.swift` plus the C module), `AppModel` (`NotesModel.swift`, `Note+Display.swift`) and the starter's `NotesModelTests.swift`.
3. `swift build --build-tests && swift test`.

Results (*measured*):

- The bindings and the view model **compiled unchanged**.
- **9 of the starter's 10 XCTests pass on Linux**, against the real Rust core, in 0.54 s. The tenth (`testScenePhasesMapOntoAppPhases`) maps SwiftUI's `ScenePhase` and can only run where SwiftUI exists.
- The test file needed three changes: `Darwin.` → `Glibc.` for its socket helper, dropping `import SwiftUI`, and `@MainActor` on one test (Linux XCTest does not infer it where macOS does; strict concurrency is the same on both).
- Cold build 14 s; a boundary mistake (calling `addNote(body:)` where Rust exposes `addNote(text:)`) reported as `error: incorrect argument label in call (have 'body:', expected 'text:')` **in 1.2 s**, against about 35 minutes through macOS CI.
- The `ForgeServices+*.swift` adapters (Keychain, notifications, sharing) import Apple-only frameworks and were left out; they would need to stay in the SwiftUI target or behind `#if canImport`.

What it does not cover: SwiftUI views, UIKit, and Apple-only frameworks. Those still need xtool's Darwin SDK (the laptop) or Xcode (macOS CI).

How far the examples could use it today, counting Swift files that import no Apple UI or device framework: Counter 625 of 750 lines, Weather 948/2016, Chat 628/2466, Device Lab 259/1180, Pocket Library 249/1918. The starter's discipline (model files import Foundation and Observation only) has drifted in the bigger apps; it is cheap to restore and worth making a rule.

## The gaps, ranked by friction

Friction = how often it bites × what it costs each time. "Agent" = blocks an AI agent driving the loop unattended.

### 1. Swift mistakes surface late, and not at all in the cloud — every Swift change; ~35 min each in the cloud. Agent: **yes** (cloud), partial (laptop)

On the laptop, `forge run ios` catches them in a minute or two. In a cloud coding session there is no Darwin SDK (Xcode's licence doesn't let us redistribute the SDK into the environment), so the only Swift compiler is macOS CI, which queued 22–29 minutes today because several threads share the macOS runners and the macOS workflow has no path filter.

Smallest fixes, in order:

1. **`forge check swift` on Linux** (new, small): build the app's host Rust library, then compile `Generated/ForgeFFI` and the app's Foundation-only Swift files and run the XCTests that don't need SwiftUI, with the stock Linux Swift toolchain. Proven by the spike above. Template change: split `Sources/AppHost` into `AppModel` (Foundation, Observation) and `AppHost` (SwiftUI), as the starter already does in spirit. Run it in the Linux Rust CI job too (seconds, no queue).
2. **Path-filter the macOS workflow** to Swift, templates, bindings and the crates they link, so docs-only PRs (like this one) don't wait in the same queue.
3. **Parse-check the SwiftUI files** on Linux (`swiftc -parse`): catches syntax errors only, but costs nothing.
4. For SwiftUI type errors from the cloud: run `forge build ios` on the laptop through Remote Control, as threads already do by hand. A self-hosted GitHub runner on the laptop would make that automatic, but the repo is public, so a fork's PR could run code on the laptop; only consider it with "require approval for all outside collaborators" and `workflow_dispatch`, and not before the other three.

### 2. Every install ends with a tap — every iteration. Agent: **yes**

`xtool launch` is broken on iOS 17+ (upstream issue #44). But `idevice` 0.1.68, already pinned by forge-device, can launch through CoreDevice's app service once the personalised developer disk image is mounted, and PR #61 already mounts it for the UI-test runner.

Smallest fix: `forge device launch <bundle-id>` (and `terminate`), reusing #61's DDI mount; then `forge run ios --launch` and `forge run ios --devtools --launch --attach` (install, launch, pair if needed, stream `forge device connect`). That one command is the loop. It also handles the bundle-id rewrite xtool does (`XTL-<team>.<id>`), which #61 already found.

### 3. Nobody can see the UI from the computer — every UI change. Agent: **yes**

No `#Preview` on Linux, and the simulator screenshots run only in macOS CI, for three apps. The phone is the only renderer.

Smallest fixes:

1. **`forge device screenshot`**: a developer screenshot service needs only the mounted DDI, not the WebDriverAgent runner, so it costs no app slot. PNG to a file, so a person or an agent can look at it. Which service answers on iOS 26 needs checking on the phone: the lockdown `screenshotr` service, or CoreDevice's screen capture service, which idevice also supports.
2. **`forge ui tree`** from #61 (WDA, accessibility hierarchy with identifiers) for structure. It needs the runner app, which uses one of the three slots.
3. A one-command **light/dark/largest-text set on the phone**: set the appearance through `forge device cmd` (the app can apply `preferredColorScheme` and a Dynamic Type override in a dev build), screenshot after each. This replaces the CI simulator screenshots for any app, not just three.

### 4. Logs are incomplete and hard to filter — every diagnosis. Agent: partial

- The live stream (`forge device connect`) carries only Rust's `forge_runtime::logging`. Swift `print`, `Logger` and caught Swift errors are not in it ("Swift-side logs not included", ledger). The fix is already planned as audit item C3 (a Swift log bridge into the same records); it should move up.
- Every `forge init` app runs as `AppHostXtool`, so `forge device logs --process` and `crashes --app` can't tell two apps apart. Fix: let `forge init` name the xtool product after the app (`MyAppXtool`), as Chat does.
- `forge device connect` has never run on the phone (ledger, M1 "iPhone: No"). Until it has, the main diagnosis tool is unproven. Run it at the next phone session.

### 5. The phone's state breaks the loop at random — weekly or worse; minutes to an hour each. Agent: **yes** when it needs hands on the phone

The 3-app limit, the 7-day expiry, the DDI unmounting after a reboot, usbmuxd going stuck after a USB dropout, the Local Network permission after a reinstall, and the 10 App IDs a week. Each is diagnosed after the fact today.

Smallest fixes:

1. **`forge device apps`**: the sideloaded apps on the phone (installation_proxy) with each profile's expiry date (misagent). Answers "which slot can I free?" and "what expires before the demo?" before the install fails. `forge device apps --remove <id>` frees a slot without touching the phone (destructive, so a person approves it; an agent must ask).
2. **Mount the DDI automatically** in any command that needs it (launch, screenshot, UI tests), with one clear message when it can't.
3. **`forge doctor --device`** that checks usbmuxd responds, the phone is paired and unlocked, Developer Mode, DDI, free slots and expiry in one go, and tells you to replug when usbmuxd is stuck. Most of the checks exist in pieces (`ios_env.rs`, forge-device).

What stays human: tapping Trust, turning on Developer Mode, trusting the profile once, Face ID or passcode. Those are once per phone or per Apple account, so an agent can run unattended after a supervised first session.

### 6. Build and install are slower than they need to be — every iteration. Agent: no

Stock xtool relinks on every run (13.1 s no-change rebuild vs 2.9 s with the patched build), re-plans the package, re-creates the provisioning profile online and uploads the full IPA. The patched xtool (fork branch `ixos`, AppImage in project files) fixes the first two. Smallest fix: make the patched build the documented default in linux-setup.md, and record install time in `forge run ios` output so we can see where the rest goes. Profile reuse is upstream xtool Tier 2 work (on the fork only, per Reuben).

### 7. Crashes can't be read and there's no debugger — rare, but expensive. Agent: partial

`forge device crashes` pulls reports, unsymbolicated. Smallest fix: `--symbolicate`, with `llvm-symbolizer` against the binary in the last build (the build already has its debug info; the Swift toolchain ships llvm-symbolizer). A Linux debugger (lldb + debugserver through idevice's debug proxy) is possible but not small; leave it until symbolicated crashes and the live log aren't enough.

### 8. Commands speak to people, not programs — every agent step. Agent: **yes**

Only `forge doctor --json` is machine-readable. Build failures end in a "What went wrong" block of text. Smallest fix: `--json` on `build`, `run`, `device *` and `test`, reusing `diagnose.rs`'s layer and fix fields, and stable exit codes per layer. The MCP thread owns the shape of this; it is listed here because humans' scripts need it too.

### 9. Automated tests stop at the Rust boundary — every regression. Agent: partial

Rust tests are fast and many. XCTest runs only on macOS, though the spike shows most of the model tests could run on Linux (gap 1). On-device UI tests are PR #61 (draft): WDA over idevice, accessibility identifiers, TOML scripts. It costs one of the three app slots, so a free account effectively has two app slots while UI testing. Nothing to add beyond finishing #61; gap 5's `forge device apps` makes the slot cost visible.

### Smaller things found on the way

- troubleshooting.md still says apps from `forge init` have only been built, never installed, and that the current template has not been built with xtool. Pocket Library (a `forge init` app) was built with xtool on the laptop on 2026-11-07. Update once the starter runs (iphone-acceptance.md A3–A5).
- The "build without the maintainer" DX exercise in the ledger has never run. Worth running once gaps 1–3 are fixed, since it measures exactly this page.
- Device Lab (#59) is a capability test bench (sensors, connectivity, Bluetooth), not a loop tool. It benefits from every fix here like any app.

## Proposed order

Each item is a small PR on existing crates. None adds a layer.

| # | Fix | Gap | Builds on | Rough size |
|---|---|---|---|---|
| 1 | Path filter on the macOS workflow | 1 | macos.yml | Hours |
| 2 | `forge check swift` on Linux + `AppModel`/`AppHost` split in the template + Linux CI step | 1 | This spike | 1–2 days |
| 3 | `forge device launch`/`terminate`, auto DDI mount, `forge run ios --launch --attach` | 2, 5 | #61's DDI mount, forge-device | 1–2 days |
| 4 | `forge device screenshot` (screenshotr) and the appearance set | 3 | #3's DDI mount, forge-devtools commands | 1 day |
| 5 | Swift log bridge (audit C3) and per-app xtool product names | 4 | forge-devtools, forge-runtime logging | 1–2 days |
| 6 | `forge device apps` (slots, expiry, remove) and `forge doctor --device` | 5 | forge-device | 1 day |
| 7 | `--json` and exit codes on build/run/device/test | 8 | diagnose.rs; shape agreed with the MCP thread | 1–2 days |
| 8 | Patched xtool as the documented default; install time in the output | 6 | xtool fork | Hours |
| 9 | Crash symbolication | 7 | forge-device | 1 day |

After 1–4, the loop is: edit, `forge check swift` (seconds), `forge run ios --launch --attach` (a minute or two), `forge device screenshot`, read the log. A person or an agent can do that with nobody at the phone, once the phone has been set up.

**What this means for the other threads** (no changes made to them): the extraction work's C3 (Swift log bridge) moves up, because the loop needs it. The Bevy trial doesn't depend on any of this, but it would get launch and screenshots for free. The MCP server should wrap items 2, 3, 4, 6 and 7 rather than reimplement them; without them an MCP server would have nothing an agent can use unattended.

## How to reproduce the measurements

- macOS CI: GitHub Actions run 42386067119 (push to main at fac9435): jobs created 15:22, started 15:44 and 15:51, finished 15:55 and 16:00 UTC.
- Rust loop: `forge init LoopApp`, then `cargo test --manifest-path rust/Cargo.toml` after touching `lib.rs`.
- Swift on Linux: the three-step recipe in the spike section, with `docker run --rm -v $PWD:/w -w /w swift:6.2 swift test`, and `-L` pointing at the host `libforge_ffi.a`.
- Laptop build times: [ACCEPTANCE.md for Chat](../examples/chat/ACCEPTANCE.md) (D2, D14), [Pocket Library](../examples/pocket-library/ACCEPTANCE.md) (step 5), and the xtool fork's laptop results (patched vs stock no-change rebuild).
