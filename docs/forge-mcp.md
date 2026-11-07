# Forge MCP: an iOS development environment an AI agent can operate

Status: design, 2026-11-07. Nothing here is built. One throwaway spike was run to answer the SDK question (see [Rust MCP SDK](#rust-mcp-sdk)); it is not in the repository.

The idea (Reuben, 2026-11-07): Forge becomes an MCP server, so a coding agent can **create, build, install, launch, inspect, test and fix a real native iOS app on a physical iPhone, from Linux, without a person in the loop**. The agent still writes Rust and SwiftUI with its own editing tools. Forge gives it the parts of iOS development an agent can't do alone: the toolchain, the signing, the phone, and eyes on the result.

This page is the MCP layer only. The loop itself (create, change the UI, run on the iPhone, inspect, diagnose, repeat) and its gaps are assessed in [dev-loop.md](dev-loop.md) (PR #67). Gaps there are cited here as **DL1** to **DL9**. Almost every blocker for an agent is a blocker for a person too, and is fixed in the CLI first.

## The short answer

- **It is feasible, and most of the hard parts already exist.** `forge init`, `forge build ios`, `forge run ios`, failure diagnosis (`diagnose.rs`), `forge device` (info, logs, crashes, live session with probes and commands) and, in PR #61, WebDriverAgent over `idevice` (accessibility tree, tap and type by identifier, screenshots) cover most of what the agent needs. The MCP server is a thin front end on top of them.
- **Four gaps block an unattended agent today**, and all four are already on the dev-loop list: nothing launches the app after install (DL2), nothing shows the screen (DL3), the phone's state (slots, expiry, disk image, USB) is only discovered by failing (DL5), and commands print prose, not data (DL8). M0 is mostly closing those four in the CLI, then wrapping them.
- **Where the agent runs decides how fast it can fix Swift.** On Reuben's laptop, the xtool build reports a SwiftUI error in a minute or two. In a cloud session there is no Darwin SDK, so SwiftUI errors only surface through macOS CI, about 35 minutes later (DL1). M0 therefore runs the agent **on the laptop**, talking to Forge over stdio.
- **The SDK question is settled.** The official Rust SDK, `rmcp` 3.9.0 (Apache-2.0, MSRV 1.88, protocol 2026-07-28), built inside our toolchain pin, passed the licence list in `deny.toml`, and served a `forge doctor` tool and the capability ledger as a resource to a scripted client in the spike.
- **Safety is enforced by the server, not by trusting the agent.** Reading and building are free. Anything that spends a scarce phone resource, deletes data or touches the Apple account needs a person's approval, through MCP elicitation, and the server refuses when it can't ask.

## What Forge MCP is, and what it isn't

| It is | It isn't |
|---|---|
| Another front end to the same operations as the `forge` CLI, in the same binary (`forge mcp`) | A replacement for the CLI, or a second implementation of anything |
| Tools that operate the environment: build, sign, install, launch, observe, drive the UI | A code generator. There is no "create screen" tool: the agent writes SwiftUI itself, guided by [ios-ui.md](ios-ui.md) |
| Self-description: what Forge can do, what is proven on an iPhone, how to use it | A copy of the docs. Resources are the docs and the ledger, served as they are |
| Usable by any MCP client (Claude Code, Cursor, Codex, others) | Tied to one agent or one model |

The design rule from the framework principles still holds: Forge's value is the native iOS environment on Linux. MCP makes it operable by programs as well as people. Every capability added to Forge appears to the agent without a new integration, because the MCP surface is generated from the same operations and the same docs.

## Where it runs

The server must run on the machine the iPhone is plugged into, because it needs usbmuxd, xtool's signing session and the Darwin SDK. That is Reuben's laptop. The agent can sit in one of three places:

| Topology | Swift feedback | Phone access | Verdict |
|---|---|---|---|
| **A. Agent on the laptop, Forge MCP over stdio** (Claude Code or any client starts `forge mcp`) | xtool build on the laptop: 1–2 min clean, seconds for a no-change rebuild with the patched xtool (DL6) | Direct | **M0.** No network listener, nothing to authenticate |
| B. Cloud agent, Forge MCP on the laptop over Streamable HTTP through a tunnel | Same as A | Through the tunnel | Later, if at all. A network-reachable server that can sign and install apps needs OAuth and a tunnel. Remote Control already gives cloud threads a laptop session, which covers the same need |
| C. Cloud agent, no laptop | Foundation-only Swift in seconds with `forge check swift` (DL1's spike: 9 of 10 starter XCTests on stock Linux Swift). SwiftUI only via macOS CI, ~35 min | None | Useful for model and Rust work, not for the device loop |

For an agent's fix loop the difference between A and C is the whole story: a SwiftUI typo costs one minute in A and 35 in C. That is why M0 is topology A, and why `forge check swift` (DL1 fix 1) is still worth doing: even on the laptop it is faster than an xtool build for model-layer mistakes, and it is the only Swift check a cloud agent gets.

## Architecture: MCP wraps the CLI

```
coding agent (Claude Code, …) ── edits files with its own tools
   │ MCP over stdio
forge mcp                       tools, resources, prompts; policy; one device queue
   │ spawns itself:  forge <cmd> --json      (stdout = one JSON result, stderr = progress)
forge CLI                       init, config, build, run, device, ui, check, doctor
   ├─ xtool                     Swift build, signing, install
   ├─ forge-device              usbmuxd: info, logs, crashes, apps, launch, screenshot, live session
   ├─ forge-uitest (#61)        WDA runner: tree, tap, type, scripts
   └─ diagnose.rs               known failures → stage, problem, fix
iPhone (Cati)
```

Decisions:

1. **Same binary, new subcommand: `forge mcp`.** No version drift between what the agent calls and what a person runs. A separate `forge-mcp` crate holds the server code; `forge-cli` depends on it.
2. **Each tool runs the matching CLI command as a child process with `--json`.** Three reasons. Today's commands print to stdout as they go, and on a stdio transport stdout *is* the protocol, so one stray `println!` would corrupt the session. A child process can be killed when the client cancels a call (`notifications/cancelled`) or a timeout passes, which matters for xtool installs that hang when the phone drops off USB. And the JSON results are then the public contract for scripts and CI too, not something only MCP gets. Moving operations into a library with an event sink can come later, behind the same JSON shapes, once they settle.
3. **The `--json` result shape is defined here** (DL8 left the shape to this thread). See [The result contract](#the-result-contract).
4. **One device queue.** `rmcp` runs tool calls concurrently (the spike answered a slow call after a fast one). usbmuxd, the WDA runner and xtool installs should not be interleaved, so every tool that touches the phone takes a per-UDID lock. Read-only project tools run freely.
5. **The server is stateful where it helps the agent**: the last build's result, the installed bundle id (xtool rewrites it to `XTL-<team>.<id>`), the log position at the last launch. That is what lets `forge_read_logs { since: "launch" }` and `forge_diagnose` work without the agent passing ids around.

## The tools

Grouped as Reuben proposed. "Exists" means the operation is on `main` or in an open PR; "Gap" names the missing piece. The **M0** column is what the first milestone needs.

### Project

| Tool | What it does | Built on | Gap | M0 |
|---|---|---|---|---|
| `forge_create_app` | New app from the starter: Rust crate, UniFFI bindings, SwiftUI shell, `forge.toml` | `forge init --path` | `--json` result (paths, bundle id, product name). Per-app product name instead of `AppHostXtool` (DL4) | Yes |
| `forge_inspect_app` | One call that answers "what is this project": `forge.toml` settings, Rust exports, Swift files split into model and view, accessibility identifiers in use, modules enabled, last build | `forge config show` (text), `forge bindings generate` | New: a `forge inspect --json` that gathers these. Mostly reading files | Yes |
| `forge_configure_app` | Change bundle id, display name, permissions, entitlements, minimum iOS, then regenerate Info.plist, entitlements and xtool.yml | `forge.toml` + `forge config sync` (M4, #31) | The agent edits `forge.toml` itself; the tool only runs `sync` and `check` and returns the diff. Entitlement changes are policy-gated (below) | Sync only |
| `forge_check` | Fast checks without a device: Rust tests, binding drift, Foundation-only Swift on Linux | `forge test`, `forge check` | `forge check swift` (DL1 fix 1) | If DL1 lands |

### UI

| Tool | What it does | Built on | Gap | M0 |
|---|---|---|---|---|
| `forge_inspect_ui` | The real accessibility tree of the screen on the phone: element type, identifier, label, value, enabled, frame. Plus unlabelled controls | `forge ui tree` (#61, WDA `/source` → `UiNode`) | `--json`. Needs #61 merged and its first phone run | Yes |
| `forge_screenshot` | PNG of the phone's screen, returned as image content (downscaled for the model) and saved to a file | `forge ui screenshot` (#61, WDA); `forge device screenshot` without the runner (DL3 fix 1) | DL3 fix 1 | Yes |
| `forge_ui_audit` | Accessibility audit of the current screen: missing labels, no identifier, duplicates, small tap targets, truncated text at large sizes | #61 M3 plan | Not built | No (M2) |
| `forge_set_appearance` | Light, dark, largest Dynamic Type, then screenshot | DL3 fix 3 (devtools command in dev builds) | Not built | No (M1) |

There is deliberately no tool that "creates screens". The agent writes SwiftUI, and Forge tells it how through resources ([ios-ui.md](ios-ui.md), the starter's patterns, the identifier conventions from #61).

One limit to say plainly: **"inspect the SwiftUI hierarchy" means the accessibility hierarchy**, not the SwiftUI view tree. From outside the app, on a device, without a debugger, the accessibility tree is what exists. It has element types (button, text field, cell, navigation bar, switch), which is what "identify native controls" needs, and it is the same tree VoiceOver uses, so an agent that makes it clean also makes the app accessible. A SwiftUI view dump would need code in the app (a devtools command calling `Self._printChanges()` or a reflection walk); not proposed until the accessibility tree proves insufficient.

### Framework

| Tool | What it does | Built on | Gap | M0 |
|---|---|---|---|---|
| (resource) `forge://capabilities` | What exists, what is proven on an iPhone, limits | [capability-ledger.md](capability-ledger.md), Modules table (#39, #58, #63) | A machine-readable form (below) | Yes (Markdown) |
| (resource) `forge://docs/{page}` | How to use each capability: native services, storage, networking, notifications, permissions, app config | docs/ | None | Yes |
| `forge_add_capability` | Turn on a module for an app: Cargo feature, `forge.toml` permission keys, Swift service adapter | Nothing yet. Swift-side service selection is audit item C7, a prerequisite | Not built. Until C7 lands, the agent follows the doc page | No (M2+) |

Adding storage, networking, notifications or permissions today is a matter of editing `Cargo.toml`, `forge.toml` and Swift files, which the agent can do with its own tools once the docs tell it how. A tool only earns its place when the steps are mechanical and easy to get wrong, which is exactly what C7 will make them.

### Build

| Tool | What it does | Built on | Gap | M0 |
|---|---|---|---|---|
| `forge_build_ios` | Rust for iOS, bindings, Swift build with xtool. No phone needed. Returns diagnostics with file, line, column and a stage | `forge build ios`, `diagnose.rs` | `--json` with parsed diagnostics: cargo `--message-format=json` for Rust, `file:line:col: error:` lines for swiftc (DL8) | Yes |
| `forge_diagnose` | Correlates the last build, install, launch, log lines since launch, new crash reports and the last UI test failure into one report with the likely layer and fix | `diagnose.rs` (build and install only), `forge device crashes` | Runtime correlation is new. M0 version: last failure + logs since launch + new crashes, no inference | Basic |

Signing happens inside `forge run ios` (xtool signs online with the logged-in Apple ID). There is no separate "sign" tool, and changing the signing identity is not exposed at all (see safety).

### Device

| Tool | What it does | Built on | Gap | M0 |
|---|---|---|---|---|
| `forge_device_status` | Connected phones; for the chosen one: unlocked, Developer Mode, disk image mounted, free app slots, profile expiry per app, usbmuxd health | `forge device list/info`, `ios_env.rs`, `forge doctor` | `forge device apps` and `forge doctor --device` (DL5 fixes 1 and 3) | Yes |
| `forge_install_device` | Build, sign, install the project's app | `forge run ios` (`xtool dev run`) | `--json`; a timeout and a clear "phone dropped off USB" result | Yes |
| `forge_launch_app` / `forge_terminate_app` | Start or stop the app on the phone | Nothing on main. `idevice` CoreDevice app service, or WDA `launch` from #61 | DL2 (`forge device launch`, with automatic DDI mount) | Yes |
| `forge_remove_app` | Delete an app from the phone to free a slot | Nothing yet | DL5 fix 1 (`--remove`). Always needs approval | No (M1) |

### Inspection

| Tool | What it does | Built on | Gap | M0 |
|---|---|---|---|---|
| `forge_read_logs` | Bounded log read: `since` (launch, a time, a cursor), `limit`, `grep`, `source` (system log or the app's own live stream) | `forge device logs`, `forge device connect` (#57) | Logs stream until Ctrl-C today. Needs `--since`/`--for` so a call ends, and the server keeps a capture running from the last launch. Swift logs missing from the live stream (DL4, audit C3) | Yes (system log) |
| `forge_read_crashes` | New crash reports for the app since the last launch | `forge device crashes --app --pull` | Symbolication (DL7) | Yes |
| `forge_app_state` | The app's own status, probes and commands, in a dev build | `forge device status/snapshot/cmd` (#57) | Pairing needs the app on screen within two minutes of launch: fine for an agent that just launched it, but never run on the phone yet | No (M1) |

`forge_app_state` is the most interesting inspection tool for an agent: it sees the Rust state directly ("3 notes, last error none") rather than inferring it from pixels. It arrives in M1 because the live session has not yet run on Cati.

### Testing

| Tool | What it does | Built on | Gap | M0 |
|---|---|---|---|---|
| `forge_ui_act` | One interaction, by locator: tap, type, clear, swipe, scroll to, accept or dismiss an alert, background for N seconds | `forge-uitest` `Driver` (#61) | A one-step CLI (`forge ui do`) or the server calling the crate | Yes (tap, type) |
| `forge_run_ui_test` | Run a TOML test file; returns per-step results, screenshots, the tree at the failure, nearest matches for a missing locator | `forge ui run` (#61) | `--json` report (#61 M1 already plans `report.json`) | Yes |
| `forge_write_ui_test` | (no tool) The agent writes TOML files with its own tools; the format is a resource | #61 | None | n/a |

Locators are identifiers first, then labels, never coordinates. That rule from #61 is what keeps agent-written tests from breaking on every layout change, and it is enforced by the tool schema: `forge_ui_act` has no x/y parameters.

## The result contract

Every command the server wraps gets `--json`. Stdout carries exactly one JSON object; progress and raw tool output go to stderr and to a log file the result points at. Proposed shape, version 1:

```json
{
  "schema": "forge.result/1",
  "command": "run ios",
  "ok": false,
  "stage": "swift",
  "duration_ms": 41210,
  "diagnostics": [
    {"severity": "error", "source": "swiftc", "file": "Sources/AppHost/TimelineView.swift",
     "line": 42, "column": 17, "message": "value of type 'Post' has no member 'likeCount'"}
  ],
  "hints": [
    {"stage": "swift", "problem": "Swift calls a Rust API that changed.",
     "fix": "Run `forge bindings generate`, then rebuild."}
  ],
  "device": {"udid": "…", "name": "Cati"},
  "app": {"bundle_id": "org.forgemobile.agentlab", "installed_as": "XTL-…org.forgemobile.agentlab"},
  "log": ".forge/logs/2026-11-07T16-40-12-run-ios.log"
}
```

- `stage` reuses `diagnose::Stage` (rust, swift, signing, device) plus `test` and `ok`. Exit codes follow it, so shell scripts can branch without parsing: 0 ok, 2 usage, 10 Rust, 11 Swift, 12 signing, 13 device, 14 test failure.
- `hints` are `diagnose.rs` matches, unchanged.
- The MCP tool returns this object as `structuredContent` with an `outputSchema`, plus a short text summary for clients that ignore structured content.
- Raw output is never dropped: it is in the log file, and `forge_diagnose` can return its tail.

**Implemented (Phase 0.5)** as `forge --json` (`crates/forge-cli/src/report.rs`), with these details settled while building it:

- Two fields beyond the sketch above: `error` (the human-readable message, present only on failure) and `data` (each command's own payload: `init` gives the path, `config show` the effective settings, `device list`/`info` the phones, `device crashes` the reports and pulled files, `device logs` and the live commands `data.lines`, `run ios` `installed`/`launched`).
- Two more stages: `usage` (exit 2: the command was called wrongly, for example `device logs --json` with neither `--for` nor `--limit`) and `forge` (exit 1: anything unclassified, such as a bad `forge.toml`). Device commands that fail without a recognised cause count as `device` (13); `forge test` and `forge check` as `test` (14).
- `app` is filled in whenever the command works on an app directory: name, bundle id, xtool product and path. `installed_as` (xtool's `XTL-…` rewrite) is not known yet; it needs the launch work (DL2).
- The log file is `<app>/.forge/logs/<UTC time>-<command>.log`, written for `build ios`, `run ios` and `test`, starting each step with `$ <command line>`.
- `forge doctor --json` keeps its own report shape, which predates this contract.
- `forge device logs --for <DURATION>` ends a log read even when the phone is silent. A `--since` that reaches back in time was not built: the USB syslog relay is live-only, so history before the call has to come from a capture the MCP server keeps running from the last launch.

## Resources and prompts: making Forge self-describing

Resources are read-only context the agent can load before acting. They are the answer to "it shouldn't rediscover the framework every time".

| Resource | Content | Source |
|---|---|---|
| `forge://capabilities` | Every module and capability: provided by, native APIs, iOS status, proven on an iPhone or not, limits | capability-ledger.md (Modules table and capability rows) |
| `forge://docs/{page}` | app-crate, ios-ui, native-services, app-config, forge-device, forge-ui-testing, troubleshooting, linux-setup | docs/ |
| `forge://conventions` | The short list an agent must follow: model files import Foundation and Observation only; identifiers on every interactive element (`screen.element`); labels for people; no coordinates in tests; logs through `forge_runtime::logging` | Distilled from ios-ui.md, #61, DL1 |
| `forge://project/config` | Effective `forge.toml` and generated settings for the current project | `forge config show --json` |
| `forge://device` | The status from `forge_device_status`, cached | forge-device |
| `forge://runs/{id}` | A UI test or diagnose report, with screenshots | `forge ui run --out` |
| `forge://limits` | What a free Apple account and Linux can't do (below), so the agent plans around them | capability plan, this page |

The ledger is Markdown tables today. Serving it as-is works for M0 (models read tables well). For M1 it should become data the agent can query ("is local notifications proven on iPhone?") without reading 200 lines. Decided: one record file per capability generates the Markdown (open decision 4).

Server `instructions` (sent at connect) are short: read `forge://conventions` and `forge://capabilities` first, check `forge_device_status` before installing, never remove apps or change signing without asking.

Prompts are reusable task recipes a person can pick in their client:

| Prompt | Steps it lays out |
|---|---|
| `new_app` (name, idea) | Read conventions and capabilities, create the app, keep the model Foundation-only, add identifiers as you go, build, install, launch, screenshot, inspect the tree, write one UI test |
| `fix_build` | Read the last result's diagnostics, fix the earliest error first, rebuild; if the stage is signing or device, check device status before touching code |
| `ui_review` (screen) | Screenshot in light, dark and largest text, inspect the tree, compare against ios-ui.md, fix, repeat |
| `prove_capability` (name) | Use a capability in the app, run it on the phone, collect logs and a screenshot as evidence for the ledger. A person still records the ledger result |

MCP's newer "skills" extension (SEP-2640, supported by `rmcp` 3.9 as `enable_skills`) is a natural later home for these recipes. Prompts are enough for M0 and every client supports them.

## Safety boundaries

The phone, the Apple account and the free-team quotas are shared, scarce and partly irreversible. Annotations like `destructiveHint` are only hints to the client, so the server enforces the policy itself.

| Tier | Examples | Rule |
|---|---|---|
| **0. Observe** | doctor, inspect app, capabilities, device status, logs, crashes, screenshot, UI tree, app state | Always allowed |
| **1. Reversible, inside the project** | create app (new folder), config sync, bindings, build, check, run UI tests and UI actions in the project's own app, install over an already-installed copy of the project's app, launch, terminate | Allowed, inside the MCP roots the client declared (or the directory `forge mcp` started in) |
| **2. Spends or loses something** | Installing an app whose bundle id is not on the phone yet (a new App ID out of 10 per week, and one of 3 slots); removing an app (its data is gone); mounting the disk image the first time after a reboot (calls Apple's TSS server); changing entitlements; pairing devtools; app commands the app marks destructive | Needs a person's approval. The server asks through **MCP elicitation** with a plain question ("Install org.forgemobile.agentlab? It uses the last free app slot."). If the client can't elicit, the call fails with the reason and the CLI command a person can run |
| **3. Never through MCP** | Apple ID sign-in, xtool auth, certificate revocation, changing team or signing identity, reading Keychain items, pairing keys or xtool's tokens, installing an arbitrary IPA, anything outside the project roots, arbitrary shell | Not exposed. A person does these in a terminal |

More rules:

- **A standing allowance** can be given at start: `forge mcp --allow install-new --bundle-id org.forgemobile.agentlab` lets the agent install that one app freely. That is how M0 avoids an approval on every run.
- **Device allowlist.** The server only touches phones named in the project's or user's config (`[mcp] devices = ["Cati"]`), never "whatever is plugged in".
- **Untrusted content.** Log lines, crash reports, UI text and screenshots come from the app, and a feed in an "X clone" can contain anything. The server marks them as app output in the result. Because tier 2 and 3 are enforced in the server, text that tricks the agent still can't remove an app or touch the account.
- **Transport.** M0 is stdio only: no listening socket. If topology B is ever built, it is Streamable HTTP bound to localhost behind a tunnel with MCP's OAuth, and tier 2 still needs elicitation.
- **No upstream xtool changes** are needed for any of this. Fixes to xtool go on Reuben's fork.

## What a paid team would unlock

Everything on this page works on the free Apple ID team Reuben uses today, and that stays the default: Forge must work for anyone with a free Apple ID. A paid Apple Developer Program membership (US$99 a year) is an open decision. This is what it would change for an agent.

| | Free team (today) | Paid team |
|---|---|---|
| Apps on a phone | 3 sideloaded at a time | No limit |
| New App IDs | 10 per 7 days | No weekly limit |
| Profile lifetime | 7 days | 1 year, never past the membership's end |
| Agent bundle ids | One sandbox id, `org.forgemobile.agentlab`, shared by every agent-made app | One per app (for example `org.forgemobile.agent.tipster`), each with its own data, side by side |
| WDA runner (#61) | Takes one of the 3 slots and is reinstalled weekly | Installed once a year |
| Paid-only capabilities | xtool drops them when it signs | Push notifications, Sign in with Apple, associated domains (universal links, passkeys, password autofill), network extensions, Wi-Fi Aware, SiriKit, multipath. xtool turns on the App ID capability from the app's entitlements |
| Distribution | Your own devices only | TestFlight and the App Store become allowed. Whether xtool can produce a distribution build is unchecked |
| Unattended sign-in | Apple ID and password session | Also an App Store Connect API key, which doesn't depend on an Apple ID session |
| Devices | Your own | Up to 100 iPhones a year registered with the team |

What it would mean for the agent loop:

- **Phone state stops being the common failure.** Most of the "Practical limits" rows below are free-team limits: slot juggling, weekly re-signing, the App ID budget. With a paid team, `forge_device_status` only needs to watch the membership's end date.
- **The tier-2 policy shrinks.** Installing a new app under the agent prefix becomes tier 1, so an agent can keep several experiments on the phone without asking. Removing apps, entitlement changes and sending pushes stay tier 2; APNs keys and API keys are tier 3.
- **The X clone gets real push and sign-in.** APNs needs a paid team, and so do Sign in with Apple and passkeys (passkeys and universal links also need an HTTPS domain serving an `apple-app-site-association` file). On the free team the M2 app uses local notifications from background refresh and a username and password.

**Recommendation:** pay before M2 (the mock social app), not before M0. M0's Tipster fits the free team: the runner and the sandbox take two slots and the run is shorter than a week. M2 is where the free team starts to cost real work (slot management across runs, no push, no Sign in with Apple), and paying then removes a whole class of failures the agent would otherwise need rules for. If Reuben pays, the switch is `xtool auth login` with the paid team chosen and `team_type = "paid"` in `forge.toml` ([ios-signing.md](ios-signing.md#moving-to-a-paid-team)); this page's tier and bundle-id rules then follow the paid column.

xtool facts above are from its source (main branch, 2026-11-07): `Sources/XKit/DeveloperServices/App IDs/` (capabilities from entitlements, paid-only ones dropped for free teams), `Sources/XKit/Utilities/ProvisioningIdentifiers.swift` (App IDs are `XTL-<id>.<bundle id>` on every team), `Sources/XToolSupport/AuthCommand.swift` (team chosen at sign-in). Nothing here has been run with a paid team.

## Practical limits an agent must work around

| Limit | Effect on an unattended agent | What Forge MCP does |
|---|---|---|
| **3 sideloaded apps per phone (free account)** | Cati holds Pocket Library and, once #61 runs, the WDA runner. An agent that makes a new bundle id per app fills the phone in one session | One **agent sandbox bundle id** (`org.forgemobile.agentlab`) that every agent-made app installs as, until a person promotes the app to its own id. `forge_device_status` shows free slots before any install; removing an app is tier 2 |
| **10 new App IDs per 7 days** | Same cause: a new bundle id per experiment exhausts the week | Same fix: reuse the sandbox id. The server counts new App IDs it caused and warns |
| **7-day signing** | Apps, including the WDA runner, stop launching after a week. Looks like a crash to an agent | `forge_device_status` reports each profile's expiry (misagent, DL5); launch failures with an expired profile are reported as `stage: signing`, with "reinstall" as the fix |
| **USB dropouts** (usbmuxd stops; installs hang until replug and unlock) | A hung call holds the agent forever | Every device tool has a timeout and is killed on cancel. The result is `stage: device`, `"problem": "the iPhone dropped off USB"`, `"needs_person": true`. The agent must stop and say so, not retry in a loop. This is the one failure that always needs hands |
| **Developer disk image** (personalised on iOS 17+, gone after every reboot, needs Xcode.xip and network for Apple's TSS) | Launch, screenshot and UI tests all need it | Mounted automatically by any tool that needs it (DL5 fix 2), from the image `forge ui setup` extracted once. Missing image → a person runs setup once |
| **Swift errors only on macOS CI** (from a cloud session) | A 35-minute fix loop for any SwiftUI mistake | Topology A for M0. `forge check swift` for the model layer anywhere (DL1). Path-filtering the macOS workflow (DL1 fix 2) shortens the queue for everyone |
| **No launch after install** (xtool can't launch on iOS 17+) | Every iteration ends waiting for a tap | `forge device launch` (DL2) before M0 |
| **One-time human steps** | Trust this computer, Developer Mode, trust the developer profile, unlock with Face ID or passcode | Done once in a supervised first session. `forge_device_status` reports a locked phone as `needs_person` |
| **Linux build limits** | No asset catalogs, precompiled Metal, storyboards, `#Preview` on Linux; paid-only entitlements (push, NFC, associated domains) | Listed in `forge://limits`, so the agent plans around them instead of discovering them in a build |

## Rust MCP SDK

| Option | State (2026-11-07) | Verdict |
|---|---|---|
| **`rmcp`** (modelcontextprotocol/rust-sdk, official) | 3.9.0, released 2026-11-05, Apache-2.0, MSRV 1.88, ~46 M downloads. Protocol versions 2024-11-05 to 2026-07-28. Tools with `#[tool]` macros and schemars schemas, structured content and output schemas, resources, prompts, elicitation, progress, cancellation, tasks, the skills extension; stdio and Streamable HTTP transports | **Use it.** Official, current, has everything the design needs |
| `rust-mcp-sdk` (rust-mcp-stack) | 2.0.0, August 2026, ~340 k downloads | Fine, but no reason to prefer a community SDK over the official one |
| `ultrafast-mcp`, `mcp-server` 0.1 | Stale (2025) | No |
| Hand-written JSON-RPC | | No: the protocol has moved four versions in two years |

Spike (scratchpad, not committed): an `rmcp` 3.9.0 stdio server with `default-features = false, features = ["server", "macros", "transport-io"]`, two tools and one resource:

- It built on our pinned toolchain (Rust 1.97.0) with 82 crates in its tree. Every licence is MIT or Apache-2.0, inside `deny.toml`'s list. `tokio` is already in the workspace through `idevice`.
- A scripted client did `initialize`, `tools/list`, `tools/call` and `resources/read`. `forge_doctor` ran `forge doctor --json` as a child process and returned it as structured content; `forge://capabilities` returned the ledger; `forge_read_logs` returned `isError: true` with the CLI's own usbmuxd message (the container has no phone), which is exactly the error path the agent needs.
- Calls run concurrently (the slow doctor call was answered after the fast log call), which is why the design has a per-device lock.
- Two details for the real crate: set `serverInfo` (it defaulted to "rmcp"), and set `mimeType` on resource contents, not just in the listing.

## M0: an agent builds and fixes an app on Cati, unattended

Reuben's milestone: connect an MCP-compatible coding agent to Forge, ask it to create a small native SwiftUI/Rust app, and have it build, install, inspect and fix that app on his physical iPhone without anyone intervening in the development loop.

### Done when

On the laptop, with Cati plugged in and unlocked, Claude Code (or another MCP client) is started with `forge mcp` configured and given one prompt, for example:

> Build a tip calculator called Tipster: a bill amount field, tip percentage presets, split between N people, and the per-person total. Keep the maths in Rust. Make sure it works at the largest text size.

The run passes when the transcript and the phone show all of this, **with no person touching the phone or the terminal between the first install and the end**:

1. The agent read `forge://conventions` and `forge://capabilities` and created the app with `forge_create_app`.
2. It built, installed (as the sandbox bundle id) and launched the app with Forge MCP tools.
3. It inspected the running app with a screenshot and the accessibility tree.
4. It found **at least one real problem from device evidence** (a build diagnostic, something wrong in the tree or screenshot, a log line, or a failing UI step) and fixed it, then rebuilt and checked again.
5. It wrote and ran a UI test file that types a bill amount and checks the total by identifier, and it passed on the phone.
6. Reuben looked at the app on the phone afterwards and agreed it works.

Allowed beforehand: freeing a slot, the one-time trust steps, `forge ui setup` (disk image and runner), starting the client with `--allow install-new --bundle-id org.forgemobile.agentlab`. A USB dropout during the run is recorded, not hidden: the run is repeated, and the dropout result must have told the agent to stop.

### What has to exist first

| # | Piece | Where | Size |
|---|---|---|---|
| 1 | PR #61 merged and its M0 run done on Cati (tree, screenshot, tap, type, TOML runner) | UI testing thread | In progress |
| 2 | `forge device launch` / `terminate`, auto-mount of the disk image (DL2, DL5 fix 2) | Dev-loop order #3 | 1–2 days |
| 3 | `forge device apps` (read only) and `forge doctor --device` (DL5) | Dev-loop order #6 | 1 day |
| 4 | `--json` and exit codes on `init`, `build ios`, `run ios`, `device *`, `ui *`, `config show` (DL8, shape above) | Dev-loop order #7, shape from here | 1–2 days |
| 5 | Bounded logs: `forge device logs --since/--for` | forge-device | Hours |
| 6 | `forge inspect --json` | forge-cli | Half a day |
| 7 | The `forge-mcp` crate and `forge mcp`: the M0 tools below, the four resources, two prompts, the tier policy with elicitation and `--allow`, the device lock, timeouts and cancellation; tested against fake CLI results without a phone | New | 2–3 days |
| 8 | A per-app xtool product name in `forge init` (DL4), so logs and crashes can be filtered by app | forge-cli template | Hours |

Items 2–6 and 8 make the CLI better for people with no MCP at all. Item 7 is the only MCP-specific work.

M0 tools (15): `forge_create_app`, `forge_inspect_app`, `forge_configure_app` (sync only), `forge_build_ios`, `forge_install_device`, `forge_launch_app`, `forge_terminate_app`, `forge_device_status`, `forge_screenshot`, `forge_inspect_ui`, `forge_ui_act`, `forge_run_ui_test`, `forge_read_logs`, `forge_read_crashes`, `forge_diagnose` (basic). M0 resources: `forge://conventions`, `forge://capabilities`, `forge://docs/{page}`, `forge://limits`. M0 prompts: `new_app`, `fix_build`.

### Phone budget for M0

Cati allows three sideloaded apps. M0 needs the WDA runner (from #61) and the sandbox app: two slots, leaving one (Pocket Library). Anything else installed has to be removed first; that is Reuben's call on the day.

### How it's run

Through Remote Control on the laptop, as the acceptance runs already are: a session starts the MCP client there with the prompt, records the transcript and the tool results, and Reuben only does the on-phone checks at the end. A dry run first with a fake device (the server's test mode) checks the agent loop without spending a slot.

## After M0

| | Goal | Done when |
|---|---|---|
| M1 | The agent sees state and appearance | `forge_app_state` (devtools pairing, probes, commands) after #57's first phone run; `forge_set_appearance`; Swift logs in the live stream (C3); ledger served as data; `forge_remove_app` behind approval |
| M2 | A bigger app, end to end | A mock social app (feed, profiles, posts, likes, replies, image attachments, persistent local accounts, pull to refresh) built from one prompt plus follow-ups like "make the timeline less cramped", reusing Chat's attachment and storage pieces. `forge_ui_audit` clean on its main screens |
| M3 | Capabilities as tools | `forge_add_capability` once C7 (Swift service selection) exists; `prove_capability` prompt produces ledger evidence |
| M4 | Remote agents | Only if needed: Streamable HTTP with OAuth through a tunnel, or keep using Remote Control |

The mock social app in M2 is an acceptance exercise for the agent loop, not a new demo; the six-demo sequence stays as it is.

## Open decisions

1. **Agent sandbox bundle id.** Decided (Reuben, 2026-11-07): yes. Recommend `org.forgemobile.agentlab`, one slot, reused by every agent-made app until a person promotes it. The alternative (new id per app) burns slots and App IDs within a day.
2. **Subprocess now, library later.** Recommend subprocess with `--json` for M0. A library API comes when the JSON shapes have settled and the commands stop printing to stdout.
3. **Approval route.** Recommend elicitation plus a narrow `--allow` at startup. Some clients don't support elicitation yet; they get a refusal with the command a person can run.
4. **Ledger as data.** Decided (Reuben, 2026-11-07): one record per capability, with the Markdown tables generated from the records and checked in CI, because many parallel PRs edit the ledger. `forge://capabilities` reads the records.
5. **Is M0 the next thing after #61 and the dev-loop fixes?** Decided (Reuben, 2026-11-07): yes. The CLI items in M0's table are already on the dev-loop order. Adding item 7 costs 2–3 days on top.
6. **Paid Apple Developer Program?** Open (Reuben, 2026-11-07: "could be convinced"). The design above assumes the free team. See [What a paid team would unlock](#what-a-paid-team-would-unlock) for the case and the recommendation.

## Risks

- **Client support varies.** Structured content, elicitation and image results are in the spec but not in every client. Every result also has a text form, and every tier-2 refusal says what to run instead.
- **The agent's own judgement.** An agent can loop on a failure it can't fix (a signing error, a stuck USB). Results carry `needs_person`, the `fix_build` prompt says to stop on device and signing stages, and the server refuses to retry an install more than twice in a row without a change.
- **#61 has not run on a phone yet.** If `idevice`'s XCTest support doesn't work on iOS 26.3, the tree and UI actions move to a fallback (go-ios `runwda` to start the runner, our WDA client unchanged). Screenshot through the developer screenshot service (DL3 fix 1) doesn't depend on WDA.
- **Liquid Glass and SDK stamping.** Linux builds may run in compatibility mode on iOS 26 ([ios-ui.md](ios-ui.md)). Screenshots an agent compares must come from the same kind of build.
- **Scope creep.** The point of M0 is one small app built and fixed unattended. Tools beyond the M0 list wait until a run shows the agent needs them.
