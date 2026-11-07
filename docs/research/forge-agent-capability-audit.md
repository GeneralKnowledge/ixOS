# Forge for AI agents: strategic capability and missing-ideas audit

Status: investigation, 2026-11-07. Nothing in this report has been built. It changes no code and starts no workstream. It is written to sit beside, not replace, the two design documents already in review: [dev-loop.md](https://github.com/GeneralKnowledge/ixOS/pull/67) (PR #67, gaps DL1 to DL9) and [forge-mcp.md](https://github.com/GeneralKnowledge/ixOS/pull/69) (PR #69, the MCP layer and its M0). Where this report agrees with them it says so in one line and moves on; its job is to find what they don't cover.

The question (Reuben, 2026-11-07): what would it take for an AI agent, told "Build me an X clone", to create a genuinely native SwiftUI + Rust app, build it on Linux, put it on his iPhone, inspect it, test it, find problems and fix them, with Forge supplying the environment?

How claims are marked:

| Mark | Means |
|---|---|
| **[main]** | Checked in the code on `main` at `0302192` |
| **[PR #n]** | Checked in that open PR's branch |
| **[source]** | Checked in the source of an external project (version given) |
| **[docs]** | Taken from an external project's own README or docs, not run |
| **[proposed]** | An idea in this report. Not built anywhere |
| **[device]** | Needs a run on a physical iPhone before anyone relies on it |

---

## 1. Executive summary

**Forge is closer to the vision than it looks, and the remaining gap is not "more MCP tools".** PRs #61, #67 and #69 already describe a sound agent loop: build, install, launch, screenshot, accessibility tree, tap by identifier, logs, crashes, a JSON result contract and safety tiers. Nothing here argues against them. Finish them in the order #67 proposes.

What they leave open is what this report is about. Five things decide whether an agent can build an X-like app reliably, and only the first is on anyone's list today:

1. **Swift feedback speed.** Covered by DL1 (`forge check swift`) and topology A in #69. Agreed, no more to add.
2. **Seeing the app without spending the phone.** Every proposed inspection route goes through the WebDriverAgent runner, which costs one of three free app slots, needs the developer disk image, and expires weekly. This report found three routes that don't (below).
3. **Verification as evidence, not as a passing step.** "Verify that a feature works" has no definition yet. The capability ledger already has the right philosophy ("an API is not proof"); it has not been applied to apps.
4. **The phone as a shared, scarce resource.** Three slots, seven-day expiry, ten App IDs a week, USB dropouts, one phone across many parallel agent threads. #69's sandbox bundle id helps; the account itself is the bigger lever.
5. **Knowledge the agent carries between sessions.** There is no `AGENTS.md`, no `CLAUDE.md`, no project-scoped MCP config in the repo or in what `forge init` generates **[main]**. Every new agent session starts by rediscovering Forge.

### The discoveries

These are the findings that are not in any existing plan, PR or doc. Each is expanded in section 6.

**D1. Apple's Accessibility Inspector service is reachable from Linux, without WDA.** The on-device audit daemon (`com.apple.accessibility.axAuditDaemon.remoteserver`) that Xcode's Accessibility Inspector uses can be driven over the developer services. pymobiledevice3 11.50.0 implements it **[source]**: `run_audit` with Apple's own audit types (contrast, Dynamic Type, clipped text, element detection, sufficient element description, hit region), `iter_elements` (walk every accessible element on screen), `element_attributes`, `perform_press` (activate an element), and `settings` / `set_setting` (change the phone's accessibility settings, such as text size, system-wide). That is Apple's audit, the element tree and a way to press controls, **with no runner app, no slot and no XCTest**. The `idevice` crate Forge pins does not implement this service; its DTX remote-server code is the base a port would build on. **[device]**: whether it answers on iOS 26.3 from Linux, and whether it needs the mounted disk image, is unverified.

**D2. A dozen agent capabilities are a feature flag away.** `forge-device` enables four `idevice` features (`ring`, `usbmuxd`, `syslog_relay`, `crashreportcopymobile`) **[main]**. The same pinned version, 0.1.68, contains **[source]**: DVT `condition_inducer` (Network Link Conditioner and thermal profiles, `enable_condition` / `disable_condition`), `location_simulation` (`set`, `clear`), `graphics` (FPS sampling), `sysmontap` (per-process CPU and memory), `process_control` (`launch_app`, `kill_app`, `disable_memory_limit`), `notifications`, `energy_monitor`; CoreDevice `app_service` (`launch_application`, `list_processes`, `uninstall_app`, `send_signal`), `hid` (`tap`, `drag`, keyboard, buttons), `pasteboard_service` (`get`, `set_text`, `set_image`), `orientation_service` (`rotate`), `screencaptureservices`; and lockdown `house_arrest` (`vend_container`), `installation_proxy`, `misagent`, `screenshotr`, `springboardservices`. For an agent, "test the X clone offline", "test the feed on a slow 3G link", "measure scrolling FPS on the timeline", "rotate", "paste an image" and "pull the app's database" are each one Forge command over code that already exists in the dependency tree.

**D3. Previews on the phone, not on the Mac.** Linux has no `#Preview` and no Xcode canvas. Xcode 26.3's own MCP server gives agents a `RenderPreview` tool **[docs]**; Forge can't. But a development build can render its own registered screens: host each preview in an offscreen window, apply trait overrides (light, dark, every Dynamic Type size, right-to-left), snapshot with `drawHierarchy`, write PNGs into the app container, and pull them with `house_arrest`. One install produces the whole appearance matrix for every screen, with fixture data from Rust, and it costs no extra slot. EmergeTools' SnapshotPreviews (MIT) already discovers previews at runtime from the binary and has an in-app `PreviewGallery` **[docs]**; it is the reference implementation to adopt or copy. **[device]**, and whether the `#Preview` macro expands in an xtool build is unverified (its plugin ships with Xcode, not the SDK).

**D4. The app minus pixels already runs on Linux.** Rust core: yes, in seconds. Swift view model: yes, DL1's spike ran 9 of the starter's 10 XCTests on stock Linux Swift **[PR #67]**. Put those together and an agent has a headless twin of the app: it can drive the real view model against the real Rust core from requirement-level tests in about a second. Add `house_arrest` and a bug seen on the phone can be reproduced on Linux by pulling the app's actual database into a Rust test. Nobody has framed it this way; it changes where most verification should happen.

**D5. "Does it work?" needs an evidence model.** Proposed: requirements are written as scenarios with ids; each run produces an evidence bundle (screenshots, accessibility tree, audit result, Rust probe values, log slice, build id); `forge verify` reports which requirements have current passing evidence on a device. This is the capability ledger's principle applied to apps, and it is what turns "the agent says it's done" into something Reuben can check in a minute.

**D6. The Swift–Rust boundary is invisible at runtime.** When a Swift call into Rust fails or is slow, nothing records it. A development-only trace of every UniFFI call (name, duration, error variant), carried on the existing devtools stream, would let an agent answer "what did tapping Like actually do?" without a debugger. It also gives the performance ledger its "Rust-to-Swift call overhead" figure, which is "Not measured" today **[main]**.

### Where this report challenges the current direction

- **Decide the paid Apple Developer Program now, not "before Relay".** For an agent-built X clone, the free team blocks push notifications (APNs), Sign in with Apple, associated domains and passkeys, and its 3-app, 7-day, 10-App-ID limits are the most frequent reason a loop stops. #69's sandbox bundle id and the WDA runner already use two of the three slots. US$99 a year removes more agent blockers than any single engineering item in this report. Keep Linux-first and free-team support as the floor; stop letting the free team set the ceiling.
- **The capability ledger is the repo's biggest merge-conflict hotspot and is not machine-readable.** Nine of the thirteen open PRs edit `docs/capability-ledger.md`, and six edit `docs/framework-extraction-audit.md` **[PR diffs]**. It also contains contradictory duplicate rows today: "Permission interface" appears twice (once "Partial: notifications, location, motion, Bluetooth", once "Partial: notifications only"), and so do "Photos picker" and the microphone/camera permissions row **[main]**. #69 recommends parsing the Markdown; this report recommends the opposite: one small data file per capability, with the Markdown generated. Parallel agents writing to one table is exactly the failure mode.
- **Don't build a separate UI inspection stack per route.** WDA (#61), the accessibility audit service (D1), CoreDevice HID and the in-app gallery (D3) should feed one `UiNode` model and one `Locator` vocabulary, which #61 already defines. Choose the route per operation, behind the `Driver` trait.
- **Don't adopt mobile-mcp, Appium or Maestro as Forge's device layer.** Each needs macOS or Xcode for real iPhones today **[docs]**. Keep them as compatibility targets (WDA started by Forge can be reused by Appium via `webDriverAgentUrl`), not dependencies.

### What to build next, in one paragraph

Finish the in-flight order (#61's phone run, DL1 to DL8, #69 M0). In parallel and cheaply: generate agent instructions and MCP config with every app (A1), add the idevice features in D2 behind small `forge device` commands (A3), and run the D1 spike on Cati at the next phone session. Then build the evidence model (D5) and the on-phone preview gallery (D3), because those two are what make the M2 "mock social app" acceptance run verifiable rather than anecdotal. Decide the Apple account question before M2.

---

## 2. Verified assessment of the current implementation

### 2.1 What is on `main` (0302192)

| Area | What exists | Size | Evidence |
|---|---|---|---|
| CLI (`forge`) | `doctor [--json]`, `check`, `init`, `config sync/check/show`, `test`, `bindings generate`, `build`, `devices`, `install`, `run`, `device list/info/logs/crashes/pair/unpair/status/snapshot/cmd/connect` | 7,471 lines | `crates/forge-cli/src/main.rs` |
| Failure diagnosis | Known build and install failures mapped to stage, problem and fix | 329 lines | `crates/forge-cli/src/diagnose.rs` |
| `forge.toml` app config | Info.plist, entitlements, xtool.yml generated from one file | 1,484 lines | `crates/forge-cli/src/app_config.rs` |
| Runtime | `background`, `EventHub`, `Lifecycle`, `app_shell!`, logging | 1,434 lines | `crates/forge-runtime` |
| Native services | Keychain, directories, file import, sharing, notifications, permissions, deep links; optional location, motion, Bluetooth, connectivity, device status | 5,082 lines (Rust + Swift) | `crates/forge-services` |
| Storage | Atomic writes, versioned JSON, SQLite with migrations, FTS5, recovery | 669 lines | `crates/forge-storage` |
| Networking | `HttpClient`, streams, cancellation, file upload/download, ureq + rustls | 2,313 lines | `crates/forge-net` |
| Forge Device | Device info, system log, crash reports over usbmuxd; live encrypted session with probes and commands | 837 + 1,258 + 752 lines | `forge-device`, `forge-devproto`, `forge-devtools` |
| Apps | Counter, Weather, Chat (+ server, CLI, core), Pocket Library, Device Lab, Android examples | 10,362 lines of Swift outside `Generated/` | `examples/` |

The Rust side is genuinely solid: typed errors, crash-safe persistence, cancellation, bounded event streams, and tests that run in seconds on Linux. The native side is idiomatic SwiftUI with a clear standard ([ios-ui.md](../ios-ui.md)). That is the right foundation and nothing here recommends replacing it.

### 2.2 Claims checked against the code

| Claim | Verdict | Evidence |
|---|---|---|
| `forge run ios` does not launch the app | **True** | `commands.rs:906-922`: "Open the app from the iPhone's home screen (xtool does not launch it)" |
| Only `forge doctor` speaks JSON | **True** | `main.rs:29`, `main.rs:321-322` are the only `json` flags |
| No accessibility identifiers in any app | **True on main** | `grep accessibilityIdentifier` over all Swift: 0 hits. #61 adds the first ones, to Pocket Library |
| The macOS workflow runs on every PR | **True** | `macos.yml` and `rust.yml` have no `paths` filters; `android.yml` and `screenshots.yml` do |
| Every `forge init` app is the process `AppHostXtool` | **True** | `templates/ios-app/Package.swift:23,56` |
| The devtools live session can carry Swift data | **False** | Probes are `Fn() -> String` and commands return `Reply::Done(String)` (`forge-devproto/src/message.rs`); registration is Rust-only (`templates/app-rust/src/lib.rs:412-445`). Swift can neither register a probe nor send binary data such as a PNG |
| Re-pairing needs a relaunch | **True** | `PAIRING_WINDOW = 120 s` after app start (`forge-devtools/src/lib.rs:45-47`) |
| forge-device uses only a few idevice services | **True** | Four features enabled (`forge-device/Cargo.toml:14-19`); #61 adds `xctest`, `wda`, `mobile_image_mounter`, `tss`, `core_device_proxy` |
| Swift adapters are shared code | **Partly** | They are copied into each app's `Generated/ForgeFFI/` by `scripts/generate-bindings.sh` (for example `examples/device-lab/Generated/ForgeFFI/ForgeServices+Motion.swift`). Shared source, per-app copies, no Swift package |
| The docs describe the current architecture | **Mostly** | `docs/ideal-design.md` still has sections for `forge-core` and `forge-ffi`, both deleted by R1 (#41). `chat-demo.md`, `project-generation.md` and `app-crate.md` mention them too (some intentionally, as history) |
| The ledger is consistent | **No** | Duplicate rows with different values: "Permission interface", "Photos picker", "Microphone, camera … permissions" (`docs/capability-ledger.md:112-118`) |
| Agent-facing project instructions exist | **No** | No `AGENTS.md`, `CLAUDE.md` or `.mcp.json` anywhere in the repo or templates |

### 2.3 What is proven on the phone

From the ledger and acceptance records: HTTPS and SSE (Weather, Chat), JSON storage (Chat), device info, system log and crash reports from Linux (Forge Device M0), Chat M0 and M1 acceptance on Cati. Not proven on a phone: SQLite (Chat M2 device run skipped), the live devtools session, every `forge-services` capability outside the services check app, the starter's acceptance steps, Pocket Library's device steps, and all of #61. The pattern DL already named is real: **the code is ahead of its proof, and every proof costs Reuben's hands.** That is the strongest argument for the agent loop, and also its main risk: the loop's own tools are among the unproven rows.

---

## 3. Open PRs and ongoing work

Thirteen open PRs on 2026-11-07, all touching the agent vision directly or indirectly.

| PR | What | Relation to the vision | Verdict |
|---|---|---|---|
| #61 (draft) | Forge UI Testing: `forge-uitest`, WDA over idevice, `forge ui setup/tree/screenshot/run`, TOML tests, first identifiers | The agent's eyes and hands. Core | **Prioritise.** Its first phone run is the most important unknown in the project. Keep the `Driver` trait as the seam for D1 and HID routes |
| #67 | Dev-loop assessment (DL1 to DL9) | The loop itself | **Agree.** Its order (path filter, `forge check swift`, launch, screenshot, Swift logs, `device apps`, `--json`) is right. This report adds items, it doesn't reorder |
| #69 | Forge MCP design: `forge mcp`, rmcp 3.9, tiers, sandbox bundle id, M0 | The agent interface | **Agree, with three changes** (section 4.5): workflow-gated tool sets, a run journal as the server's state, and the ledger as data files rather than parsed Markdown |
| #57 (merged) / Forge Device | Live devtools session | The agent's view of app state | Extend: Swift-side probes and binary payloads (section 6, A6) |
| #59 (merged) / #63 | Device Lab, optional device services; ledger row | Capability bench | Fine. Device Lab is also the natural test app for D2's location and condition commands |
| #64 | Managed files (`FileStore`) in forge-storage | Infrastructure an X clone needs (image attachments) | Merge as planned |
| #65 | Backoff in forge-net | Offline delivery | Merge as planned |
| #66 | `ForgeLog`: Swift logs into the Rust pipeline | Closes DL4's biggest hole | **Prioritise.** Agents diagnose from logs; Swift being silent is the largest blind spot |
| #68 | `ForgeImages`: shared ImageIO thumbnails | X clone image timeline | Merge as planned |
| #70 | Chat notifications through `LocalNotifier` | Notifications | Merge as planned |
| #60 (draft) | Chat M4 voice messages on `ForgeAudio` (403 lines in `forge-services/swift`) | Media | See overlap below |
| #53 (draft) | Pocket Library EPUB reader and read-aloud (`ReadAloud.swift`, 353 lines) | Media, documents | See overlap below |
| #55 (draft) | Local AI plan and L0 check app (own workflow `ai-check.yml`) | App capability, not dev tooling | Independent. Not on the agent's critical path |
| #62 | Kataster (Bevy) trial plan | Side experiment | Keep it a side experiment, as Reuben said. It benefits from launch and screenshot without anything game-specific in Forge |

### 3.1 Overlaps and competing approaches

- **Two owners of the audio session.** #60 adds a framework `ForgeAudio` (audio session, recording, playback) in `forge-services/swift`; #53 adds Pocket Library's own `ReadAloud.swift` that also owns the audio session and Now Playing. The ledger's Audio module row still says "Pocket Library today (Swift)". iOS has one audio session per app, so both are fine alone, but the framework now has two patterns. Recommendation: merge #60 first, then have #53 (or a follow-up) move read-aloud onto `ForgeAudio`'s session ownership, keeping speech synthesis in the app until a second user appears.
- **The same files edited by several PRs.** `examples/chat/Sources/AppHost/Attachments.swift`: #60, #66, #68. `scripts/generate-bindings.sh`: #60, #66, #68. `examples/pocket-library/Sources/AppHost/LibraryView.swift`: #53 and #61. `docs/capability-ledger.md`: nine PRs. `docs/framework-extraction-audit.md`: six. Every merge will force rebases on the others. The aoqkhv series (#64, #65, #66, #68, #70) comes from one thread and is the easiest to sequence; do it in that thread's order.
- **Stale bases.** #55 branched from `50dee63` and #60 from `f09521f`, several merges behind main. Their CI results describe an older main.
- **Three screenshot routes are planned in three places.** CI simulator screenshots (`screenshots.yml`, three apps), WDA screenshots (#61), the developer screenshot service (DL3 fix 1). Add D3 (in-app snapshots) and that is four. They have different purposes (CI review aid, current screen, any screen without a runner, every screen in every appearance) and should share one output convention (`.forge/runs/<id>/screens/…png` plus a JSON sidecar) so diffing and evidence work across all of them.
- **No conflicts in direction** between #61, #67 and #69. They were written to fit together and they do.

### 3.2 Work that is sufficient and should not be rebuilt

- The app model (one Rust crate per app, `app_shell!`, `native_services!`) and the FFI policy.
- forge-storage, forge-net, Lifecycle and logging.
- `forge.toml` config generation and `diagnose.rs`.
- The devtools protocol's security model (pairing over USB, Noise, fixed command enum). Extend it; don't replace it.
- The decision to use `idevice` rather than go-ios or pymobiledevice3 as the device stack. pymobiledevice3 is the best *reference* (D1), not a runtime dependency: a Python dependency in a Rust CLI is a step backwards.

---

## 4. The MCP opportunity and a proposed interaction model

#69 designs the server well: same binary, child processes with `--json`, one device lock, server-enforced safety tiers, elicitation for anything that spends a slot. This section adds what an agent needs on top of primitives.

### 4.1 Three levels, not one

| Level | What the agent asks | Example tools | State |
|---|---|---|---|
| **Primitives** | "Do this one thing" | #69's 15 M0 tools: build, install, launch, screenshot, tree, act, logs, crashes | Exists in design |
| **Intents** | "Find out / prove this" | `forge_verify`, `forge_explain_element`, `forge_review_screen`, `forge_diagnose_runtime`, `forge_reproduce_on_linux` | **[proposed]** |
| **Knowledge** | "What can Forge do, and how?" | Resources: capabilities, conventions, limits, recipes, API lookup, project memory | Partly in #69 |

Primitives alone force the agent to orchestrate ten calls and interpret raw output for every question. Intents are where Forge's domain knowledge lives: which logs matter after a launch, which audit issue maps to which source line, what "the app crashed" looks like when the real cause is an expired profile.

### 4.2 Intent-level operations

Each one is built only from primitives that exist or are planned. None generates code.

**`forge_verify { requirement | scenario }`** **[proposed]**. Runs a scenario (a #61 TOML file, extended with requirement ids), and returns pass or fail per requirement with an evidence bundle: screenshot per step, the accessibility tree at each assertion, Apple audit issues on each visited screen (D1), probe values from devtools, the log slice between steps, the build id, device, iOS version. This is the answer to "verify that a feature actually works". See D5.

**`forge_explain_element { locator }`** **[proposed]**. "Why can't VoiceOver reach the Like button?" Combines: the node from the tree (or "not found" with nearest matches, which #61 already plans), the audit issues for that element (D1), its frame against the screen and its siblings (overlaps, off-screen, under 44×44 pt), and the **source location** of the identifier. The last one is the overlooked part: if identifiers follow #61's `screen.element` convention, a static index of `.accessibilityIdentifier("…")` call sites (a grep over `Sources/`, refreshed on build) maps any runtime element back to `file:line`. That one index turns every runtime finding into an edit location.

**`forge_review_screen { screen }`** **[proposed]**. Screenshot plus tree plus audit, in light, dark and the largest text size. With D1's `set_setting` the text size can be changed system-wide from Linux, so this works for any app, not only ones that implement a devtools appearance command (DL3 fix 3).

**`forge_diagnose_runtime`** **[proposed]**, a superset of #69's `forge_diagnose`. Correlates in time: last build, install result, launch, the run journal's UI steps, log lines, boundary-trace errors (D6), new crash reports, profile expiry, device state. Returns the likely layer (Rust, Swift, boundary, signing, device, environment) and a fix, from rules, not inference. The rules extend `diagnose.rs`.

**`forge_reproduce_on_linux`** **[proposed]**. Pulls the app container from the phone (`house_arrest`), writes a Rust test skeleton that opens that exact store and replays the failing command, and runs it. The agent then fixes the bug in a one-second loop instead of a two-minute one. See D4.

### 4.3 State: a run journal

#69 makes the server "stateful where it helps". Make that state a file the agent and Reuben can read: `.forge/runs/<timestamp>/journal.jsonl`, one line per event (build result, install, launch, UI step, screenshot path, audit result, log excerpt, probe values, crash). It gives:

- `since: "launch"` and `since: "step 4"` queries for free;
- evidence bundles for `forge_verify` (a run directory is the bundle);
- memory across sessions: a new agent can read the last run and continue;
- a review artefact for Reuben ("show me what the agent saw").

Expose it as `forge://runs/latest` and `forge://runs/{id}`.

### 4.4 Knowledge resources

Beyond #69's `capabilities`, `conventions`, `docs/{page}`, `limits`:

| Resource or tool | What | Source | Notes |
|---|---|---|---|
| `forge://recipes/{name}` | Verified patterns: paginated timeline, pull to refresh, image cell with thumbnail, compose sheet with Rust validation, offline outbox, settings screen | Code already in Chat, Pocket Library, the starter | Each recipe points to real, CI-compiled code, not prose. This is how an agent "reuses existing native components intelligently" |
| `forge_api_lookup { symbol }` | Signature and availability of an Apple API **for the SDK this machine builds with** | The `.swiftinterface` files inside xtool's Darwin SDK, already on the laptop | Ground truth for the exact SDK version, offline. The SDK can't be redistributed, so this works only where the SDK is (topology A). **[proposed]** |
| Apple documentation | Prose, HIG, WWDC transcripts | sosumi.ai MCP (`searchAppleDocumentation`, `fetchAppleDocumentation`, `fetchAppleVideoTranscript`) **[docs]** | Use it; don't build it. Recommend it in generated agent config |
| `forge://project/memory` | Decisions, known issues and conventions for this app, written by agents | A Markdown file in the app (`FORGE.md` or `AGENTS.md`) | See A1 |
| `forge://screens` | The app's screen catalogue: preview names, identifiers per screen, last snapshot per appearance | D3 and the identifier index | The agent's map of the app |

### 4.5 Changes proposed to #69

1. **Workflow-gated tool sets.** XcodeBuildMCP, the most-used iOS MCP server, enables tool groups per session (`enabledWorkflows`) and keeps session defaults (project, device, bundle id) so calls don't repeat them **[docs]**. Forge's tool list will grow past M0's 15; start with groups (`project`, `build`, `device`, `ui`, `verify`) and session defaults from day one.
2. **The run journal as the server's state** (4.3), instead of in-memory fields.
3. **Ledger as data files**, not parsed Markdown (section 8).
4. **Ship a skill with the server.** XcodeBuildMCP and sosumi both ship an agent skill that teaches the tool workflow **[docs]**. #69 already notes rmcp 3.9's skills extension; generate the same content as a Claude Code skill file and an `AGENTS.md` section so it works in clients without it.

### 4.6 What Apple's own MCP server says about the target

Xcode 26.3 ships an MCP server (`xcrun mcpbridge`) with tools for build, build log, test listing and running, diagnostics, `RenderPreview` (preview screenshots), `ExecuteSnippet` and `DocumentationSearch`; later versions add run and debug, device interaction sessions, crash logs, string catalogs and entitlements **[docs]**. It needs macOS and Xcode. It is a useful checklist of what professional agent tooling for iOS looks like:

| Xcode MCP | Forge equivalent | Status |
|---|---|---|
| BuildProject, GetBuildLog | `forge_build_ios` + `--json` diagnostics | #69 M0 |
| RunAllTests, RunSomeTests | Rust tests, `forge check swift` (Linux XCTest), `forge ui run` | DL1, #61 |
| RenderPreview | On-phone preview gallery | **D3, not planned anywhere** |
| DocumentationSearch | sosumi.ai MCP + SDK interface lookup | **Not planned** |
| ExecuteSnippet | A Linux Swift REPL against the app model (Foundation-only) | **[proposed]**, small, follows DL1 |
| InvokeDebuggerCommand | `debug_proxy` in idevice; debugserver | Experiment (section 7) |
| DeviceInteraction | WDA (#61), HID, D1 `perform_press` | #61 |
| GetTopCrashIssues | `forge device crashes` + symbolication | DL7 |
| AddEntitlement, AddInfoPlist | `forge.toml` + `config sync` | Done (M4) |
| StringCatalog tools | None. String catalogs need Xcode's compiler | Limit; use `.strings` files on Linux **[proposed check]** |

Forge can match most of this list from Linux, and it has one thing Xcode's server doesn't: a physical phone driven by the same tool calls, with Rust state visible through devtools.

---

## 5. Research: relevant projects and tools

| Project | What it is | Transferable idea | Verdict for Forge |
|---|---|---|---|
| [idevice](https://github.com/jkcoxson/idevice) (Rust, MIT) | Pure-Rust iOS device services | Already pinned. 0.1.68 contains condition inducer, location simulation, graphics FPS, sysmontap, process control, CoreDevice app service, HID, pasteboard, orientation, screen capture, house_arrest **[source]** | **Use more of it** (D2) |
| [pymobiledevice3](https://github.com/doronz88/pymobiledevice3) (Python, GPL-3.0) | The most complete open implementation of iOS device services | `developer accessibility run-audit / list-items / element-attributes / settings` over `axAuditDaemon` **[source, 11.50.0]**; DVT simulate-location with GPX playback, screenshot, sysmon, energy, core-profile-session, oslog **[docs]** | **Reference for porting D1** into idevice-based Rust. Don't depend on it at runtime. Licence (GPL) means port from the protocol, not the code |
| [Appium WebDriverAgent](https://github.com/appium/WebDriverAgent) | XCTest runner with an HTTP API | Already the #61 engine. WDA also exposes Apple's `performAccessibilityAudit` (5.15.0 added element attributes to its output) **[docs]**; endpoint path to confirm | A second audit route inside the runner #61 already starts |
| [Apple accessibility audits](https://developer.apple.com/documentation/accessibility/performing-accessibility-audits-for-your-app) | XCTest `performAccessibilityAudit`: element description, hit region, contrast, element detection, clipped text, traits, Dynamic Type **[docs]** | Layout and accessibility defects detected by Apple's own rules, not ours | Use it through D1 or WDA rather than writing `forge_ui_audit` from scratch, as #69 M2 plans |
| [Xcode MCP](https://dev.to/arshtechpro/xcode-263-use-ai-agents-from-cursor-claude-code-beyond-4dmi) (Xcode 26.3+) | Apple's agent interface to Xcode | The target tool set (section 4.6) | macOS only. Benchmark, not dependency |
| [XcodeBuildMCP](https://github.com/getsentry/XcodeBuildMCP) (MIT) | MCP server and CLI for Xcode projects | Workflow-gated tools, session defaults, a per-workspace daemon for stateful work (log capture, debugging), a bundled agent skill **[docs]** | Copy the design lessons into #69. macOS only |
| [mobile-mcp](https://github.com/mobile-next/mobile-mcp) (Apache-2.0) | MCP for Android and iOS devices | Accessibility tree first, screenshots and coordinates as fallback **[docs]** | Same principle as #61. Its real-iPhone path needs WDA built with Xcode plus go-ios, and "Linux soon" in its FAQ **[docs]**. Not a dependency |
| [Maestro](https://github.com/mobile-dev-inc/maestro) (Apache-2.0) | YAML UI flows, `maestro mcp` | Readable flow files, auto-waiting, MCP inside the CLI | "Physical iOS devices are not yet supported" **[docs]**. #61's TOML format is the same idea; borrow vocabulary (`assertVisible`, `runFlow`) if useful |
| [SnapshotPreviews](https://github.com/EmergeTools/SnapshotPreviews) (MIT) | Discovers SwiftUI previews at runtime from the binary; in-app `PreviewGallery`; snapshot tests | Previews rendered by the app itself **[docs]** | **Reference or dependency for D3.** Check that its prebuilt XCFramework links with xtool and that previews survive an xtool build **[device]** |
| [swift-snapshot-testing](https://github.com/pointfreeco/swift-snapshot-testing) (MIT) | Snapshot assertions for views | `drawHierarchy`-based rendering with trait overrides | Rendering technique for D3 |
| [odiff](https://github.com/dmtrKovalenko/odiff) (MIT) | Fast pixel diff with anti-aliasing detection, ignore regions, threshold **[docs]** | Screenshot regression | Use for visual diffs (A9). [dssim](https://github.com/kornelski/dssim) is better perceptually but AGPL-3.0, which `deny.toml` would need to allow; avoid |
| [sosumi.ai](https://sosumi.ai/) | Apple docs, HIG and WWDC transcripts as Markdown, with an MCP endpoint **[docs]** | Apple API knowledge for agents | **Recommend in generated MCP config** (A1). Unofficial; works from Linux and cloud |
| [AGENTS.md](https://agents.md) | Convention for agent instructions, nearest file wins **[docs]** | Project knowledge that every agent reads | Generate per app (A1). Claude Code reads `CLAUDE.md`, so generate both (or one that includes the other) |
| [InjectionNext](https://github.com/johnno1962/InjectionNext) (MIT) | Hot reload for Swift, including on devices **[docs]** | Change function bodies without reinstalling | Needs macOS and Xcode's build log. A Linux port is an experiment at best (section 7) |
| [netmuxd](https://github.com/jkcoxson/netmuxd) (LGPL-2.1) | usbmuxd replacement with network support, by idevice's author **[docs]** | Phone on Wi-Fi, no cable | Experiment for USB dropouts (A11) |
| [OpenSwiftUI](https://github.com/OpenSwiftUIProject/OpenSwiftUI) (MIT) | Open-source SwiftUI reimplementation | SwiftUI rendering off Apple platforms | On Linux only a stdout renderer builds; "Linux UI framework integration is not available yet" **[docs]**. **Not viable** for previews |
| [AndroidWorld](https://github.com/google-research/android_world) | Benchmark of 116 parameterised tasks across 20 apps for mobile agents **[docs]** | Measuring agent capability with repeatable tasks | Idea for a Forge agent benchmark (A10) |
| Android Studio / Layout Inspector, Firebase Robo, monkey | Live view hierarchy, automated exploration | Exploratory crawling of every reachable screen | Crawl via the tree once #61 works (section 7) |
| Expo Go / Swift Playgrounds | One host app that runs many projects | Avoids per-app installs | Not possible for native SwiftUI on iOS (no loading new native code). The sandbox bundle id is the realistic version |
| PocketBase, chat-server | Self-hosted backends | X clone needs a server | `examples/chat-server` already has accounts, rooms, messages, reactions, read state, SSE, attachments and self-signed TLS **[main]**. Reuse its patterns for the X clone's dev backend; don't add a backend framework to Forge |

---

## 6. Missing capabilities and overlooked ideas

Ids A1 to A12 are used in section 7. Each says what Forge would contribute and what already exists.

### A1. Agent instructions and MCP config generated with every app (essential, tiny)

`forge init` generates the Rust crate, the SwiftUI shell, `forge.toml` and a README **[main]**. Add: `AGENTS.md` (and `CLAUDE.md` pointing to it) with the app's structure, the conventions (model files import Foundation and Observation only; identifiers on every interactive element; logs through `forge_runtime::logging`; never hand-edit `Generated/`), the commands for the fast loop, and a "Known issues and decisions" section agents append to. Add `.mcp.json` with `forge mcp` (once it exists) and sosumi. Do the same for the ixOS repo itself: today an agent working on ixOS must read 3,200 lines of docs to learn the rules that fit in 80. This is the cheapest high-impact item in the report.

- Existing: AGENTS.md convention, sosumi MCP. Forge contributes: template files and their content.
- Size: hours. Device: no.

### A2. Accessibility audit and inspection without the runner (D1) (essential spike)

- Problem: every inspection route in #61 and #69 needs the WDA runner (1 of 3 slots, 7-day expiry, DDI), and a clean audit (`forge_ui_audit`) is planned as Forge's own rules in M2.
- Existing: Apple's audit daemon on the phone; pymobiledevice3's client as protocol reference; idevice's DVT remote-server for the transport.
- Forge contributes: a Rust client for `axAuditDaemon` (audit, element walk, attributes, press, settings) in forge-device, mapped to #61's `UiNode`.
- Unlocks: `forge_review_screen`, `forge_explain_element`, system-wide Dynamic Type for screenshots, audits on apps without identifiers.
- Limits to expect: element walk by moving the accessibility focus is slower than WDA's `/source`; no text entry; system alerts are probably out of reach. It complements WDA, doesn't replace it.
- Size: spike 1 day on the phone; client 1–2 weeks. **[device]**: the whole premise.

### A3. The rest of idevice, exposed (D2) (high value, small each)

New `forge device` subcommands, each a thin wrapper:

| Command | idevice service | Agent use |
|---|---|---|
| `forge device network <profile>` / `--clear` | DVT `condition_inducer` | Offline, 3G, lossy, high-latency: test Chat's outbox, the X clone's feed, W11 offline delivery, unattended |
| `forge device location <lat,lon>` / `--gpx` | DVT `location_simulation` | Weather, FieldKit, Device Lab, any location feature |
| `forge device perf --app` | `graphics`, `sysmontap` | FPS while scrolling a timeline, memory with 10,000 messages: the "Not measured" performance ledger rows |
| `forge device container --pull/--push` | `house_arrest` | Fixtures in, databases out (D4), screenshots from D3 |
| `forge device pasteboard` | CoreDevice `pasteboard_service` | Paste text or an image into a compose field |
| `forge device rotate` | CoreDevice `orientation_service` | Landscape checks |
| `forge device apps --remove` | CoreDevice `app_service.uninstall_app` | DL5's slot freeing (tier 2) |

Note: several of these need the personalised DDI and the iOS 17+ tunnel that #61 sets up; they should reuse its `Phone` code, not duplicate it. Size: 0.5–1 day each after #61. **[device]** for each.

### A4. Evidence-based verification (D5) (essential foundation for "verify it works")

- Problem: "verify that a feature works" has no definition. Today it means a human runbook (`ACCEPTANCE.md`), or a passing UI step that proves a button exists, not that the feature works.
- Proposal: requirements live in the app as `requirements.toml` (id, statement, scenario files). Scenarios are #61 TOML with `requirement = "TL-3"`. Each run writes an evidence bundle into `.forge/runs/<id>/` (section 4.3). `forge verify` reports per requirement: passing on device with evidence from build X, failing, or unverified. Reuben reviews evidence, not transcripts.
- Why it matters: it is the X clone's definition of done, the agent's stop condition, and the app-level version of the ledger. It also produces the ledger's "iPhone" column evidence automatically, while keeping the rule that a person confirms it.
- Existing: #61 runner and report.json plan; devtools probes. Forge contributes: the requirement file, bundle layout, `forge verify`, the MCP intent.
- Size: 1 week after #61 M1. **[device]** for the runs, not the design.

### A5. On-phone previews and the appearance matrix (D3) (high value)

- Problem: no previews on Linux; CI simulator screenshots cover three apps and only run on macOS; DL3's appearance set needs per-app code.
- Proposal: a `ForgePreviews` Swift helper in development builds (`FORGE_DEVTOOLS`): registered screens with fixture state from Rust; a devtools command `previews.render` that renders each into an offscreen window with trait overrides, writes PNGs plus a JSON manifest into the container; `forge previews pull` fetches them with `house_arrest`. Optionally adopt SnapshotPreviews' discovery and gallery.
- Why it matters: one install shows every screen in every appearance, including screens the UI test can't reach easily (error states, empty states, 10,000-item lists). It is the "consistent design quality" check, and the input to visual regression (A9).
- Size: 1 week. **[device]**, and an xtool check of `#Preview` macro expansion.

### A6. Devtools for Swift (and for bytes) (high value, small)

- Problem: probes and commands are Rust-only and string-only **[main]**. Swift state (navigation path, presented sheet, selected tab, scroll position) is invisible, and nothing binary (a PNG, a database) can travel.
- Proposal: export probe and command registration through UniFFI (`app_shell!` already emits per-app UniFFI types), add a `Reply::Bytes` or file-reference reply. Add built-in commands every app gets in dev builds: `navigate <deep link>`, `reset-data`, `seed <fixture>`, `memory-warning`, `background-task <id>` (run the BGTask handler now), `notification <payload>` (deliver a local notification). These are the triggers Xcode gets through its debugger and Linux has no way to fire.
- Size: 3–5 days. **[device]** for the session itself (#57 has not run on Cati).

### A7. Boundary tracing (D6) (high value for diagnosis)

- Proposal: in `devtools` builds, wrap every exported UniFFI method (via the existing `app_shell!` or a small attribute macro) to emit a trace record: method, arguments summarised, duration, result variant. Stream on the devtools channel; correlate with UI steps in the run journal.
- Answers: "what did tapping Like do?", "why is the timeline slow?" (a 400 ms sync call on the main actor), "which error did Swift swallow?".
- Size: 3–5 days. Device: the records, yes; the macro, no.

### A8. Headless app twin and Linux repro (D4) (high value)

- Proposal: make the DL1 split (AppModel / AppHost) the template's structure, then add `forge test model` that runs the Swift view-model tests on Linux against the real Rust core. Add `forge_reproduce_on_linux` (4.2) using A3's container pull.
- Rule that follows: views stay a function of model state, and anything an agent needs to verify about behaviour (not pixels) is a model test that runs in a second, anywhere, including cloud sessions with no SDK.
- Size: follows DL1 (1–2 days) plus 2–3 days for repro. Device: only for the pull.

### A9. Visual regression (high value once A5 exists)

- odiff (MIT) on the A5 matrix or #61 screenshots, baselines committed per app, ignore regions for dynamic content (times, avatars), results in the evidence bundle. The agent sees "Settings in dark mode at XXXL text changed by 4% in this region" instead of eyeballing.
- Caveat already noted by #69: Linux-built apps may render in an older compatibility mode on iOS 26; baselines must come from the same build kind.
- Size: 2–3 days.

### A10. A Forge agent benchmark (high value, strategic)

- Inspired by AndroidWorld. A fixed set of app prompts of rising difficulty (tip calculator, notes with search, photo journal, the X clone), each with requirements (A4). Run after major Forge changes; record success, time, tool calls, human interventions and the failure stage.
- Why: it measures whether Forge is improving for agents, which no test does today. The ledger's "build without the maintainer" exercise (never run) is the human version of the same idea.
- Size: 1 week to set up; each run costs phone time. **[device]**.

### A11. Keep the phone reachable (high value, uncertain)

- USB dropouts stop installs until replug and unlock (laptop notes). For unattended runs: detect them fast (heartbeat service in idevice), stop with `needs_person` (#69 does this), and experiment with Wi-Fi operation (netmuxd, or the iOS 17+ remote pairing over the network that idevice implements) so a cable isn't the weak point.
- Size: spike. **[device]**.

### A12. Ledger and audit as data (essential hygiene)

- One TOML file per capability (or per module) under `docs/capabilities/`, with the Markdown tables generated by a script and checked in CI. Removes the merge conflicts that hit nine PRs, makes `forge://capabilities` queryable, and makes duplicates like the current "Permission interface" rows impossible.
- Size: 1–2 days, best done between merge waves.

### Smaller findings

- **The agent sandbox bundle id needs a reset story.** Every agent app installs as `org.forgemobile.agentlab`, so its container, Keychain items and notification permissions carry over between unrelated apps. Add a `reset-data` devtools command (A6) and clear the container on promotion.
- **Identifier lint.** With identifiers as the backbone of tests, audits and source mapping, a check that every `Button`, `TextField`, `Toggle` and `NavigationLink` has one belongs in `forge check`. SwiftLint runs on Linux; a few lines of `swift-syntax` or a regex is enough to start.
- **`forge.toml` per-configuration overrides** (roadmap "Open") matter more for agents: a debug bundle id (the sandbox) and a release id in the same app.
- **Localisation.** String catalogs (`.xcstrings`) need Xcode's compiler; the Linux path is `.strings` files. Not on any list; worth one check before an agent-built app tries to localise.
- **Doc drift.** `ideal-design.md` describes deleted crates; troubleshooting.md still says `forge init` apps were never installed (DL already noted). Agents read docs literally, so stale docs cost more with agents than with people.

---

## 7. Prioritised recommendations

"DX gain" is the expected improvement to the agent loop, from 1 (nice) to 5 (changes what is possible). Complexity: S = days, M = 1–2 weeks, L = more.

### 7.1 Essential foundations

| # | Item | Problem solved | Existing project | Forge contributes | Depends on | Size | DX gain | Device |
|---|---|---|---|---|---|---|---|---|
| 1 | Finish #61 and run it on Cati | No eyes or hands on the phone | WDA, idevice | Done in the PR | DDI, a free slot | In progress | 5 | **Yes** |
| 2 | DL1 to DL8 in #67's order | Slow Swift feedback, no launch, no screenshot, prose output | idevice, Linux Swift | Small CLI commands | #61 for launch/DDI | S each | 5 | Launch and screenshot: yes |
| 3 | #66 ForgeLog | Swift silent in logs | None | In the PR | None | Done in PR | 4 | Yes |
| 4 | A1 agent instructions + MCP config | Every session rediscovers Forge | AGENTS.md, sosumi | Template content | None | Hours | 4 | No |
| 5 | #69 M0, with 4.5's changes | No agent interface | rmcp | The server | 1–3 | M | 5 | Yes |
| 6 | A4 evidence-based verification | "Done" is undefined | #61 runner | Requirements, bundles, `forge verify` | 1, 5 | M | 5 | Yes |
| 7 | A12 ledger as data | Merge conflicts, unqueryable, inconsistent | None | Files + generator | None | S | 3 | No |
| 8 | Decide the Apple account | Slots, expiry, push, sign-in block the X clone | Apple Developer Program | A decision and a doc | None | Decision | 4 | No |

### 7.2 High-value improvements

| # | Item | Problem solved | Existing project | Forge contributes | Depends on | Size | DX gain | Device |
|---|---|---|---|---|---|---|---|---|
| 9 | A2 accessibility audit service (spike first) | Inspection costs a slot; audit planned from scratch | Apple axAuditDaemon; pymobiledevice3 as reference | Rust client, `UiNode` mapping | DDI/tunnel code from #61 | Spike S, then M | 4 | **Yes, premise** |
| 10 | A3 idevice services | Network, location, perf, container, pasteboard can't be driven | idevice 0.1.68 | Thin commands | #61 Phone code | S each | 4 | Yes |
| 11 | A6 devtools for Swift and bytes + built-in triggers | Swift state invisible; background tasks, memory warnings, notifications untestable | None | Protocol and macro changes | #57 phone run | S–M | 4 | Yes |
| 12 | A8 headless twin + Linux repro | Behaviour verified slowly, on the phone | DL1 spike | Template split, `forge test model`, repro tool | DL1, A3 container | S | 4 | Pull only |
| 13 | A5 on-phone previews and appearance matrix | No previews on Linux | SnapshotPreviews, swift-snapshot-testing | Helper, command, pull | A3 container, A6 | M | 4 | Yes |
| 14 | A7 boundary tracing | Swift–Rust failures invisible | None | Macro + stream records | A6 | S | 3 | Yes |
| 15 | Identifier → source index, `forge_explain_element` | Runtime findings don't point at code | None | Index + intent tool | #61, A2 | S | 3 | Yes |
| 16 | A9 visual regression | Visual changes unnoticed | odiff | Baselines, regions, bundle | A5 or #61 | S | 3 | Baselines: yes |
| 17 | A10 agent benchmark | No measure of progress | AndroidWorld (idea) | Prompts, requirements, scoring | A4, #69 M0 | M | 3 (strategic) | Yes |
| 18 | Recipes resource | Agents reinvent timeline, compose, offline patterns | Chat, Pocket Library code | Curated, CI-compiled recipes | None | S | 3 | No |

### 7.3 Interesting experiments

| Item | Why it might pay off | Why it might not | Device |
|---|---|---|---|
| Wi-Fi operation (netmuxd / remote pairing) | Removes the cable as the most common unattended failure | Pairing and stability on Linux unproven | Yes |
| Exploratory crawler | Visit every reachable screen via the tree, audit each, find crashes and dead ends without a script (Robo-style) | Needs #61 and A2 stable first; free-text fields need fixtures | Yes |
| `forge_api_lookup` over the SDK's `.swiftinterface` | Exact API and availability for the SDK in use, offline | sosumi may be enough; the SDK isn't available in cloud sessions | No |
| Linux Swift REPL on the model layer (`ExecuteSnippet` equivalent) | Agent can probe Rust-backed model behaviour interactively | Tests already cover most of it | No |
| debugserver through idevice `debug_proxy` | Breakpoints and backtraces from Linux | Large; symbolicated crashes + traces (A7) may be enough | Yes |
| Hot reload (InjectionNext-style) | Change a view without reinstalling | Needs macOS-side compiler integration and dylib signing; very large on Linux | Yes |

### 7.4 Probably unnecessary

- **A "create screen" or code-generation tool.** Agents write SwiftUI well; Forge's value is feedback. #69 already rejects it; agreed.
- **Rendering SwiftUI on Linux** (OpenSwiftUI, stubs from Apple's interfaces). Not viable today, and A5 gets the real renderer.
- **Adopting Appium, Maestro or mobile-mcp as the device layer.** macOS/Xcode-bound for real iPhones today.
- **A Forge backend framework.** For the X clone, reuse chat-server's patterns or a self-hosted backend; Forge stays an iOS framework.
- **A cross-platform UI abstraction or game-engine support.** Out of scope by Reuben's instruction; Kataster stays a trial.
- **Topology B (remote HTTP MCP with OAuth).** Remote Control already covers cloud threads reaching the laptop.

---

## 8. Architectural implications

1. **Forge has three products, and the third is new.** The framework (crates, services), the developer environment (CLI, device, UI testing), and now the **verification layer** (requirements, evidence, journal, audits). The third should be its own small crate (`forge-verify` or inside `forge-uitest`), consumed by the CLI and MCP, not spread through commands.
2. **One device model, many routes.** `forge-device` owns the phone (selection, lock, DDI, tunnel, services); `forge-uitest`'s `Driver` and `UiNode` own UI. WDA, the audit service, HID and in-app previews are routes behind those two interfaces. #61 is the right place to put the seam; it should not become WDA-only by accident.
3. **Development builds become an instrument.** `devtools` today is a log stream and string probes. With A5–A7 it is the app's self-description: screens, state, boundary traces, triggers. Keep it compiled out of release builds (already the rule) and keep the fixed-command security model.
4. **Model/view split becomes architecture, not style.** A8 only works if model files stay Foundation-only. DL measured the drift (Chat: 628 of 2,466 Swift lines are model-only). Make it a template structure and a `forge check` rule.
5. **The ledger becomes data** (A12), with Markdown as a view. The same applies to requirements (A4): data first, so agents and CI can read them.
6. **Swift code sharing stays file-copy for now.** Adapters are copied into `Generated/` per app **[main]**; fine for an agent (it sees the code). A real Swift package becomes worth it when the helper set (ForgeLog, ForgeImages, ForgeAudio, previews) passes ten files or needs versioning.
7. **Concurrency of agents.** Many threads already work in parallel. The device lock (#69) covers the phone; A12 covers the ledger; PRs that touch shared docs should land in small, quick merges. The single phone is the real bottleneck: one agent at a time on Cati.

---

## 9. Proposed milestones

These extend #69's M0–M4 and #67's order. They don't run beside them; they slot in.

| Milestone | Contents | Done when | Device |
|---|---|---|---|
| **A0, now (parallel, no conflicts)** | A1 agent instructions + MCP config (repo and template); A12 ledger as data (between merge waves); the account decision | A new agent session in a fresh `forge init` app knows the conventions without reading docs/; the ledger has no duplicates and regenerates in CI | No |
| **#69 M0** (as designed, with 4.5) | #61 phone run, DL1–DL8, `forge mcp` | Tipster built and fixed unattended on Cati | Yes |
| **A-Spike, next phone session** | D1 audit-service spike; check `condition_inducer`, `location_simulation`, `house_arrest` on iOS 26.3 | Each answers or fails with a recorded reason | **Yes** |
| **M1+ "see everything"** | A3 commands, A6 devtools for Swift and triggers, A7 boundary tracing, A2 client if the spike passed | An agent can put the app offline, move its location, pull its DB, read Swift state and boundary traces, run Apple's audit | Yes |
| **M1++ "prove it"** | A4 requirements and evidence, A8 headless twin and repro, identifier→source index, `forge_verify`, `forge_explain_element` | Tipster's requirements show device evidence; a seeded bug is reproduced on Linux from the phone's DB | Yes |
| **M2 "X clone" (#69's mock social app)** | A5 previews and appearance matrix, A9 visual regression, recipes | The run in section 10 passes, with an evidence report Reuben can review in minutes | Yes |
| **M3 "measure"** | A10 benchmark, performance rows filled from `graphics`/`sysmontap` | Benchmark baseline recorded; performance ledger has first real numbers | Yes |

---

## 10. The ultimate demonstration: an X clone

Prompt: "Build me an X clone: timeline, posts, replies, likes, profiles, image attachments, local persistence, networking, sign-in, notifications, background refresh. Native and polished."

### What would stop it today, and what removes it

| Blocker | Today | Removes it |
|---|---|---|
| No agent interface | CLI prints prose; no MCP | #69 M0 |
| Agent can't launch or see the app | Tap needed; no screenshot | DL2, DL3, #61 |
| Swift errors take 35 min in cloud | No SDK in the cloud | Topology A; DL1 for model code |
| Can't tell if it works | Runbooks only | A4 evidence |
| Can't see every screen and appearance | Only the current screen, via the runner | A5, A2 `set_setting` |
| Offline, slow network, background refresh untested | No way to induce conditions or fire BG tasks | A3 `network`, A6 triggers |
| Swift state and boundary invisible | Rust-only probes | A6, A7 |
| Feed performance unknown | No measurement | A3 `perf` |
| **Push notifications** | APNs needs a paid team; a non-goal since 2026-11-05 | Paid account, or accept local notifications from background refresh (best effort) |
| **Sign-in** | Sign in with Apple and passkeys need a paid team (entitlements, associated domains) | Paid account, or username/password + Keychain token against the dev backend |
| Phone capacity | WDA runner + sandbox app = 2 of 3 slots | Paid account; or D1 to avoid the runner for inspection |
| A backend | None for social data | Adapt chat-server (accounts, SSE, attachments, reactions already there) |
| Image pipeline | In flight | #64 FileStore, #68 ForgeImages, Chat M3 patterns |
| Design quality | Standard exists (ios-ui.md) but nothing checks it | A5 matrix, A2/WDA audit, A9 diffs, recipes |

The hard part of the X clone is not writing it. It is the agent knowing, with evidence, that the timeline loads offline, the like survives a relaunch, the compose sheet works at the largest text size, and the image upload resumes after the network drops. Every row above except the account rows is a Forge capability, and most are small once #61 lands.

### What the run would look like with the milestones done

1. The agent reads `AGENTS.md`, `forge://capabilities`, `forge://recipes`, `forge://limits`. It sees push needs a paid team and plans background refresh instead (or not, if the account was upgraded).
2. It writes `requirements.toml` (timeline loads cached posts offline; like is optimistic and reconciles; compose validates in Rust; images thumbnail off the main thread; every screen passes Apple's audit at the largest text size).
3. It builds Rust and the view model with Linux tests until green: seconds per loop.
4. It builds and installs the sandbox app, launches it, pairs devtools, renders the preview matrix, and reviews it.
5. It runs scenarios with `forge_verify`: puts the phone on the 100% loss profile, relaunches, checks the timeline from probes and the tree, restores the network, watches the outbox drain in the boundary trace.
6. Audit finds clipped text in the profile header at AX5; `forge_explain_element` points to `ProfileHeader.swift:41`; the agent fixes it; the matrix diff confirms only that region changed.
7. It writes the evidence report. Reuben reads one page, looks at the phone, and decides.

---

## 11. What Forge could eventually enable

A Linux laptop, an iPhone on a cable (or on Wi-Fi), and an agent. A person describes an app. The agent builds a native SwiftUI app with a Rust core, using framework modules that are proven on a real phone and documented as data. It sees every screen in every appearance, reads Apple's own accessibility verdict, drives the UI by identifier, induces bad networks and moved locations, measures frame rates, reads Rust and Swift state as structured data, and reproduces device bugs on Linux in a second. It stops when the requirements have evidence, not when it runs out of ideas, and it leaves behind a report a person can check in minutes and a project memory the next agent can continue from.

Nothing in that paragraph needs a Mac, a simulator or a new UI framework. It needs the in-flight work finished, about a dozen small device commands over a library Forge already depends on, an evidence model, and one decision about the Apple account.

---

## Appendix A. Device validation needed

When these are ready to test, Reuben only needs to connect the iPhone; the commands can be run from the laptop through Remote Control.

| Item | What to learn |
|---|---|
| #61 M0 | WDA runner starts via idevice `xctest` on iOS 26.3.1; tree, tap, type, screenshot |
| D1 / A2 | `axAuditDaemon` answers from Linux; needs DDI or not; audit types on iOS 26; `set_setting` for text size |
| A3 | `condition_inducer` profiles list and apply; `location_simulation` set/clear; `house_arrest` container of an xtool-signed dev build; `graphics` FPS samples |
| A5 | Offscreen rendering of SwiftUI screens with trait overrides on device; `#Preview` expansion in an xtool build |
| #57 | First live devtools session on Cati (prerequisite for A6, A7) |
| Profile expiry | misagent expiry dates match what the phone enforces |

## Appendix B. Evidence index

- Repository: `main` at `0302192`; open PR branches as of 2026-11-07 16:20 UTC: #53 `aplud8`, #55 `412qgh`, #60 `ll349h`, #61 `e6ryft`, #62 `7ykg13`, #63 `upvy18`, #64 `aoqkhv`, #65 `aoqkhv-backoff`, #66 `aoqkhv-swift-log`, #67 `y2svc9`, #68 `aoqkhv-images`, #69 `v3v89e`, #70 `aoqkhv-notify`. No open issues.
- idevice 0.1.68 source (crates.io), `src/services/dvt/*`, `src/services/core_device/*`, `src/services/house_arrest.rs`, `src/services/wda.rs`.
- pymobiledevice3 11.50.0 wheel (PyPI): `pymobiledevice3/services/accessibilityaudit.py` (service names, `run_audit`, `iter_elements`, `perform_press`, `set_setting`, `AuditType`), `cli/developer/accessibility/`.
- External pages as linked in section 5, read 2026-11-07. Third-party descriptions of Xcode's MCP server are not Apple sources; tool names vary between Xcode versions.
