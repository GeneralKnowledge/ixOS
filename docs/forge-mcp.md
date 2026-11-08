# Forge MCP: an iOS development environment an AI agent can operate

Status: **M0 server built** (`forge mcp`, crate `crates/forge-mcp`) and **run by an agent on Cati** (2026-11-07, [first run](#first-tipster-run-on-cati-2026-11-07)): a headless Claude Code agent built, installed, inspected, fixed and UI-tested Tipster with no person touching the phone, and Reuben confirmed on 2026-11-08 that the app works. **M0 is done.** [Using Forge MCP](#using-forge-mcp) says what exists and how it differs from the design below; the rest of the page is the design it follows.

The idea (Reuben, 2026-11-07): Forge becomes an MCP server, so a coding agent can **create, build, install, launch, inspect, test and fix a real native iOS app on a physical iPhone, from Linux, without a person in the loop**. The agent still writes Rust and SwiftUI with its own editing tools. Forge gives it the parts of iOS development an agent can't do alone: the toolchain, the signing, the phone, and eyes on the result.

This page is the MCP layer only. The loop itself (create, change the UI, run on the iPhone, inspect, diagnose, repeat) and its gaps are assessed in [dev-loop.md](dev-loop.md) (PR #67). Gaps there are cited here as **DL1** to **DL9**. Almost every blocker for an agent is a blocker for a person too, and is fixed in the CLI first.

## The short answer

- **It is feasible, and most of the hard parts already exist.** `forge init`, `forge build ios`, `forge run ios`, failure diagnosis (`diagnose.rs`), `forge device` (info, logs, crashes, live session with probes and commands) and `forge ui` (#61, merged; not yet run on a phone), WebDriverAgent over `idevice` (accessibility tree, tap and type by identifier, screenshots) cover most of what the agent needs. The MCP server is a thin front end on top of them.
- **Four gaps block an unattended agent today**, and all four are already on the dev-loop list: nothing launches the app after install (DL2), nothing shows the screen (DL3), the phone's state (slots, expiry, disk image, USB) is only discovered by failing (DL5), and commands print prose, not data (DL8). M0 is mostly closing those four in the CLI, then wrapping them.
- **Where the agent runs decides how fast it can fix Swift.** On Reuben's laptop, the xtool build reports a SwiftUI error in a minute or two. In a cloud session there is no Darwin SDK, so SwiftUI errors only surface through macOS CI, about 35 minutes later (DL1). M0 therefore runs the agent **on the laptop**, talking to Forge over stdio.
- **The SDK question is settled.** The official Rust SDK, `rmcp` 3.9.0 (Apache-2.0, MSRV 1.88, protocol 2026-07-28), built inside our toolchain pin, passed the licence list in `deny.toml`, and served a `forge doctor` tool and the capability ledger as a resource to a scripted client in the spike.
- **Safety is enforced by the server, not by trusting the agent.** Reading and building are free. Anything that spends a scarce phone resource, deletes data or touches the Apple account needs a person's approval, through MCP elicitation, and the server refuses when it can't ask.

## Using Forge MCP

New apps carry it in `.mcp.json` (`forge init` writes `{"forge": {"command": "forge", "args": ["mcp"]}}`), so Claude Code and other MCP clients start it from the app folder. By hand:

```bash
forge mcp                                                    # root = current folder; installing refused
forge mcp --allow install --bundle-id org.forgemobile.agentlab   # the M0 run: the agent may install the sandbox app only
forge mcp --root ~/apps --udid 00008130-…                   # another root; device tools only touch this phone
forge mcp --no-device                                        # no phone tools at all, even with a phone plugged in
```

Built (M0), each tool running `forge <command> --json` in the app folder:

| Tool | Runs | Notes |
|---|---|---|
| `forge_doctor` | `doctor` | Includes which Apple team signs, free or paid |
| `forge_create_app` | `init --bundle-id --path` | Bundle id defaults to the sandbox `org.forgemobile.agentlab`; the folder must be new and inside the root |
| `forge_inspect_app` | `config show` + a file scan | Swift files as model or view by their imports, accessibility identifiers in use, `uitests/*.toml`, a warning for a `*Model.swift` that imports SwiftUI, the last failure |
| `forge_configure_app` | `config sync`, then `config check` | After the agent edits `forge.toml` |
| `forge_check` | `test`, then `check swift` | Says in every result that SwiftUI was not compiled |
| `forge_build_ios` | `build ios` | Diagnostics with file, line, column |
| `forge_install_device` | `run ios` | Tier 2: refused unless `--allow install` (and the bundle id is in `--bundle-id`, when given); the refusal carries `person_command` |
| `forge_device_status` | `device list` + `device apps` + `doctor` | Phones, usbmuxd, sign-in, signing team, installed apps with days left, free-team slots in use, and a note when every slot is taken or a profile has expired |
| `forge_read_logs` | `device logs --for --limit --process <xtool_product>` | 1–120 s, live only; marked as app output |
| `forge_read_crashes` | `device crashes --app --pull <app>/.forge/crashes --limit N --project <app>` | Symbolicated against the local build: kind, message, culprit frame with file and line, crashed thread |
| `forge_diagnose` | (server state) | The last failed result in the session, the end of its log file, and `data.correlation` (M6.1): the failed step or explore errors, the app's log just before, the symbolicated crash and how well (`symbolicated`), `culprit_source` (the crash's source file placed in the repository by path, never by name, and whether it changed since the server started: `true`, `false`, or `null` when that can't be told), `collection` (what the run couldn't collect), the changed files across the repository, and the likely layer. A timed-out or cancelled tool's result says `stopped: timeout` or `cancelled`, and a stopped UI run still names its `out` folder |
| `forge_ui_explore` | `ui explore --project --actions --minutes --per-screen --out --since <server start> --verify [--seed --skip --largest-text --real-data --focus --no-previous]` | M6.1. On the app's test data (or `real_data`, looking only); never `--allow-destructive` and no typed text from the agent. A bundle in `.forge/runs/<UTC>-ui-explore/` with a replay file per issue. M6.2: aimed at the files changed since the server started (`focus_changes: false` for the plain walk), learning from the previous walk (`previous`), issues replayed (`verify`, default 3); `data.focus` and `data.compare` |
| `forge_ui_focus` | `ui focus --project (--since <server start> \| --changed …)` | M6.2. Which identifiers the changed files declare and the ways into them, with how; the UI tests using them; changed files with no link and why. No phone |
| `forge_launch_app`, `forge_screenshot`, `forge_inspect_ui`, `forge_ui_act`, `forge_run_ui_test` | `ui run` with a one-step test file (`launch`, `terminate`, `tap`, `type`, `clear`, `expect`), `ui screenshot`, `ui tree --app`, `ui run` | Listed only when the `forge` binary has `forge ui` (any build from main since #61). Screenshots, and the failure screenshot of a UI run, come back as images. `forge_launch_app` takes `text_size` (`small` to `largest`): a `UIPreferredContentSizeCategoryName` launch argument, so the agent checks the largest Dynamic Type size without changing the phone's settings (not yet run on a phone), and `appearance` (`dark`, `light`; M6.3): the `-ForgeAppearance` launch argument that Forge apps honour from their root view |
| `forge_ui_audit` | `ui audit --app` | M6.3. The screen's accessibility audit without the tree: errors and warnings by issue (`no_label`, `no_identifier`, `duplicate_identifier`, `small_tap_target`, `clipped`), each with its element and what was measured, and `checks`, which says what isn't measured (contrast, truncation inside a frame). Linux-tested only |
| `forge_accept_screenshots` | `ui accept <run> [--only NAME]` | Makes a `forge_run_ui_test` run's screenshots the baselines its test is compared with. `forge_run_ui_test` compares every screenshot by default and fails on a changed screen; its result has `data.visual` and returns the first diff image ([visual-regression.md](visual-regression.md)) |
| `forge_verify` | `verify [--twin-only]`, `verify --run <server start>` | Per requirement in `requirements.toml`: the newest twin, iPhone and person verdicts and whether they are for the current files ([evidence.md](evidence.md#forge-verify)). The stop condition. With `run: true`, the run record of this session instead (`report.md`, [evidence.md](evidence.md#run-records-and-acceptance-forge-verify---run---accept)). No tool records a person's acceptance: `forge verify --accept` refuses to run under the server (`FORGE_MCP_CHILD`). `forge_run_ui_test` writes its output to `.forge/runs/<UTC>-ui-run/`, so its results count |

Resources: `forge://conventions` (the starter's AGENTS.md rules plus how to use the tools), `forge://capabilities` (the generated ledger), `forge://limits` (the practical limits below and the free/paid table from ios-signing.md), `forge://docs/{page}`. Prompts: `new_app`, `fix_build`, `fix_crash`.

**Client timeouts.** Some clients stop waiting for a tool long before Forge's own limits (30 min for a build, 8 min for an install, 5 min for `forge_create_app`). Claude Code gives up after 60 s by default (found in the Murmur dry run, 2026-11-08: `forge_create_app` "timed out" three times while `forge init` was still running, and the agent finished the folder by hand), so start it with `MCP_TOOL_TIMEOUT=1900000`. Other clients have their own setting.

Enforced by the server: paths stay inside the root (symlinks included); phone tools run one at a time; every child has a timeout, and a timeout or a cancelled call kills its whole process group (xtool and cargo too); a device tool that times out returns `needs_person: true`.

Different from the design below, on purpose for M0:

- **Approval is a standing allowance only.** Elicitation is not built: a tier-2 call without `--allow` is refused with the command a person can run. The 2026-07-28 protocol reworked server-to-client requests, and client support varies, so this waits until a client needs it.
- **Launch goes through the WebDriverAgent runner** (`launch` and `terminate` steps of a UI test file, #61), not a separate `forge device launch` (DL2). Without `forge ui setup`, nothing launches the app. On Cati (2026-11-07) this works once **Settings > Developer > Enable UI Automation** is on; without it WDA refuses with code 41 and a person has to open the app by hand.
- **No slot or expiry count yet.** `forge_device_status` reports the team and its limits but cannot list installed apps or profile expiry (`forge device apps`, DL5, not built).
- **`--udid` instead of a device allowlist** in config. `forge_install_device` cannot pass it on: xtool chooses the phone.
- **Not built:** `forge_set_appearance` (the appearance is a launch argument instead, M6.3), `forge_app_state`, `forge_remove_app`, `forge_add_capability` (M1 and later, as planned); screenshot downscaling; a server-side log capture from the last launch; the "no more than two identical failed installs" rule.

Checks (Linux, no phone): `cargo test -p forge-mcp` runs unit tests and a full MCP session over an in-memory pipe with a fake `forge` script (policy refusals, root escapes, diagnostics in summaries, a hung device call killed with `needs_person`, UI step files). Nothing has run against Cati yet; `iphone-acceptance.sh --with-mcp` (step X2) is the first phone check, asking `forge_device_status` from the phone. A short scripted session (doctor, device status, logs, then a launch, tree and screenshot) follows once `forge ui` has run on the phone; the unattended Tipster run comes after that.

### The run journal

Every tool call is appended to `<root>/.forge/runs/<UTC start>-mcp/journal.jsonl` (format `forge.journal/1`; the path is printed on stderr when the server starts, and nothing is written until the first call). The first line records the server: forge version, root, `allow_install`, `no_device`, allowed bundle ids, UDID. Then one line per call:

```json
{"event":"call","seq":7,"at":"2026-11-08T09-12-40","tool":"forge_ui_act","args":{"project":"Tipster","action":"type","id":"bill.amount","text":{"chars":2}},"ok":true,"stage":"ok","duration_ms":4210,"cli":"forge --json ui run …"}
```

A failed call adds `error` (first 500 characters), `needs_person`, the `hints` problems, and `log` and `evidence` paths when the result has them. Typed text and expected values (`text`, `value`) are kept only as their length; app output (trees, logs, screenshots) stays in the files the result points at, never in the journal. It is what a person reads after an unattended run like Tipster's below, and what a later agent reads to pick up where one stopped.

### First Tipster run on Cati (2026-11-07)

`run-tipster.sh run` on the laptop: one `claude -p` with the M0 prompt and `forge mcp --allow install --bundle-id org.forgemobile.agentlab`, from a forge build of #92 plus #93. 44 minutes, 84 turns, nobody touching the phone.

- The agent created Tipster, kept the maths in Rust, checked on Linux, built, installed, launched, and read the screenshot and tree. It drove the app (typed 84, tapped 20 %, added a person, expected £50.40 per person) and checked logs and crashes. It relaunched at `text_size: largest`, rebuilt and reinstalled, and its UI test `tip.toml` passed on the phone (14 of 14 steps in 9 s).
- Problems it found from device evidence and fixed: the keyboard covered the per-person total, so the total moved under the bill; at the largest size the presets became 350×78 rows, so it switched to a `ViewThatFits` grid; VoiceOver read the stepper twice, so it relabelled it "Number of people".
- Reuben opened Tipster on Cati the next morning and confirmed it works (done-when item 6).
- What went wrong in Forge:
  - Two of the four installs hung. xtool printed `[Installing] 100%`, never printed `[Verifying]`, and never exited. One of them had installed the app anyway. The agent waited out the full 30-minute limit before it got `needs_person`. Now `forge run ios` stops xtool after 90 s of silence following `[Installing] 100%` and reports `data.verified: false`, and `forge_install_device` gives up after 8 minutes.
  - A launch with `text_size` changed nothing while the app was running, so it now stops the app first.
  - Tap by id hit a toolbar wrapper that shared the button's identifier, there was no scroll, and `expect gone` failed on a stale element. All three are fixed in `forge ui` (#92), and `forge_ui_act` has a `scroll` action.

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
4. **One device queue.** `rmcp` runs tool calls concurrently (the spike answered a slow call after a fast one). usbmuxd, the WDA runner and xtool installs should not be interleaved, so every tool that touches the phone takes a per-UDID lock. Read-only project tools run freely. Before the first UI tool call the server runs `forge ui start --exit-with <its pid>`, so all UI tools share one WebDriverAgent runner that ends with the server ([forge-ui-testing.md](forge-ui-testing.md#one-runner-for-many-commands)).
5. **The server is stateful where it helps the agent**: the last build's result, the installed bundle id (xtool rewrites it to `XTL-<team>.<id>`), the log position at the last launch. That is what lets `forge_read_logs { since: "launch" }` and `forge_diagnose` work without the agent passing ids around.

## The tools

Grouped as Reuben proposed. "Exists" means the operation is on `main` (when this was written, some of it was still in open PRs, #61 among them); "Gap" names the missing piece. The **M0** column is what the first milestone needs.

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
| `forge_inspect_ui` | The real accessibility tree of the screen on the phone: element type, identifier, label, value, enabled, frame. Plus unlabelled controls | `forge ui tree` (#61, WDA `/source` → `UiNode`) | `--json`. #61 is merged; needs its first phone run | Yes |
| `forge_screenshot` | PNG of the phone's screen, returned as image content (downscaled for the model) and saved to a file | `forge ui screenshot` (#61, WDA); `forge device screenshot` without the runner (DL3 fix 1) | DL3 fix 1 | Yes |
| `forge_ui_audit` | Accessibility audit of the current screen: missing labels, no identifier, duplicates, small tap targets, truncated text at large sizes | `tree.rs` audit, `forge ui audit` (M6.3) | Built in M6.3, Linux-tested; truncation is measured as text off the screen or over other text, not an ellipsis inside a frame | No |
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
| `forge_launch_app` / `forge_terminate_app` | Start or stop the app on the phone | WDA `launch` step from #61 (on main, used by M0). A runner-free route is not built: `idevice` CoreDevice app service | DL2 (`forge device launch`, with automatic DDI mount) | Yes |
| `forge_remove_app` | Delete an app from the phone to free a slot | Nothing yet | DL5 fix 1 (`--remove`). Always needs approval | No (M1) |

### Inspection

| Tool | What it does | Built on | Gap | M0 |
|---|---|---|---|---|
| `forge_read_logs` | Bounded log read: `since` (launch, a time, a cursor), `limit`, `grep`, `source` (system log or the app's own live stream) | `forge device logs`, `forge device connect` (#57) | Logs stream until Ctrl-C today. Needs `--since`/`--for` so a call ends, and the server keeps a capture running from the last launch. Swift logs missing from the live stream (DL4, audit C3) | Yes (system log) |
| `forge_read_crashes` | The app's newest crash reports, symbolicated | `forge device crashes --app --pull` | Done: `forge-crash` (DL7) | Yes |
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
| `fix_crash` | Read the newest crash with forge_read_crashes, start at its culprit frame, reproduce the cause in a Linux test, fix, reinstall and check that no new report appears |
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

Everything on this page works on a free Apple ID team, and that stays the default: Forge must work for anyone with a free Apple ID. Reuben has not paid for the Apple Developer Program (corrected 2026-11-07; an earlier note here said the membership was renewed, which was wrong). On Cati, xtool signs with his free personal team. If he does pay, joining does not switch xtool to the paid team by itself (it signs with the team chosen at `xtool auth login`); `forge doctor` shows which team signs ([ios-signing.md](ios-signing.md#which-team-is-xtool-using)). This is what a paid team changes for an agent.

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
| **3 sideloaded apps per phone (free account)** | Cati holds Pocket Library and the WDA runner (installed 2026-11-07). An agent that makes a new bundle id per app fills the phone in one session | One **agent sandbox bundle id** (`org.forgemobile.agentlab`) that every agent-made app installs as, until a person promotes the app to its own id. `forge_device_status` shows free slots before any install; removing an app is tier 2 |
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
| 1 | PR #61 merged and its M0 run done on Cati (tree, screenshot, tap, type, TOML runner) | UI testing thread | Merged 2026-11-07. On Cati: setup, launch, tree and screenshot work (with #90); find by identifier and a passing M0 test are not done yet |
| 2 | `forge device launch` / `terminate`, auto-mount of the disk image (DL2, DL5 fix 2) | Dev-loop order #3 | 1–2 days |
| 3 | `forge device apps` (read only) and `forge doctor --device` (DL5) | Dev-loop order #6 | 1 day |
| 4 | `--json` and exit codes on `init`, `build ios`, `run ios`, `device *`, `ui *`, `config show` (DL8, shape above) | Dev-loop order #7, shape from here | Done (`forge --json`, `forge.result/1`) |
| 5 | Bounded logs: `forge device logs --since/--for` | forge-device | `--for` done; no `--since` (the system log relay is live only) |
| 6 | `forge inspect --json` | forge-cli | Half a day |
| 7 | The `forge-mcp` crate and `forge mcp`: the M0 tools below, the four resources, two prompts, the tier policy with elicitation and `--allow`, the device lock, timeouts and cancellation; tested against fake CLI results without a phone | **Built** (without elicitation; see [Using Forge MCP](#using-forge-mcp)) | Done |
| 8 | A per-app xtool product name in `forge init` (DL4), so logs and crashes can be filtered by app | forge-cli template | Done |

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
| M2 | A bigger app, end to end ([run plan](#m2-run-murmur)) | A mock social app (feed, profiles, posts, likes, replies, image attachments, persistent local accounts, pull to refresh) built from one prompt plus follow-ups like "make the timeline less cramped", reusing Chat's attachment and storage pieces. `forge_ui_audit` clean on its main screens |
| M3 | Capabilities as tools | `forge_add_capability` once C7 (Swift service selection) exists; `prove_capability` prompt produces ledger evidence |
| M4 | Remote agents | Only if needed: Streamable HTTP with OAuth through a tunnel, or keep using Remote Control |

The mock social app in M2 is an acceptance exercise for the agent loop, not a new demo; the six-demo sequence stays as it is.

### M2 run: Murmur

Started 2026-11-08 after Tipster passed. The app is **Murmur**, a microblogging timeline that lives entirely on the phone: local accounts (sign up, sign in, switch), a home timeline of followed accounts, compose with a 280-character count and an optional photo, likes, replies in a thread view, profiles with follow, and pull to refresh. Five simulated accounts are seeded in Rust and pull to refresh brings in new posts from them. All state is in Rust with SQLite (forge-storage).

Choices, and why:

- **No server.** A laptop server like Chat's would add Wi-Fi, IP and uptime failures that say nothing about the agent loop. The simulated accounts give the feed something to refresh.
- **Free team.** No push, no Sign in with Apple, one sandbox slot (it installs over Tipster). Nothing in the app needs a paid team, so paying is not a prerequisite for this run.
- **Short phone stages, one agent session.** Reuben (2026-11-08) didn't want one 90-minute unattended run: the phone is tied up all that time, and any Forge fix it shows has to wait for the end. So the run is staged. Everything that doesn't need the phone runs first, as long as it needs, with the server started with `--no-device`, so no phone tool is offered even if a phone is plugged in: the agent writes the app, makes `forge_check` and `forge_build_ios` (SwiftUI on the laptop) pass and writes the UI test. Each phone stage is then 15–20 minutes at most. Every stage resumes the same session (`claude -p --resume`), so the agent keeps what it learned, and Forge can be fixed and rebuilt between stages.
- **Two changes after it works.** "The timeline feels cramped" and "add bookmarks", each made without the phone and then checked on it. This checks the agent can change an app it built.
- **No `forge_ui_audit`.** It isn't built (M1). The agent checks the main screens with `text_size: largest`; the accessibility audit (`ax_probe`) runs by hand afterwards if wanted.

| Stage | Phone | Limit | What the agent does |
|---|---|---|---|
| `a-build` | No | 60 min | Reads the resources, creates Murmur, writes Rust and SwiftUI, `forge_check` and `forge_build_ios` pass, writes the UI test |
| `b-look` | Yes | 20 min | Installs, launches, screenshots and tree of the main screens at the default and largest text size, logs; fixes what they show |
| `c-test` | Yes | 15 min | Runs the UI test until it passes |
| `change-1` | No | 30 min | Less cramped timeline |
| `check-1` | Yes | 15 min | Install, look, UI test |
| `change-2` | No | 30 min | Bookmarks, and the UI test extended |
| `check-2` | Yes | 15 min | Install, look, UI test |

At most about 65 minutes of phone time in four sittings, usually less: a stage ends when its work is done. Prompts and the laptop script: [`scripts/agent-runs/murmur/`](../scripts/agent-runs/murmur/) (`run-murmur.sh prep`, then one stage at a time, `summary`).

Passes when, with no person touching the phone or terminal during any stage:

1. Everything Forge does goes through Forge MCP tools: create, check, build, install, launch, screenshots and the tree.
2. At least one real problem found from device evidence is fixed and checked again.
3. A UI test passes on the phone covering sign up, post (top of the timeline), like (count), reply (in the thread view), and a terminate and relaunch with the post still there; after change 2 it also covers bookmarks.
4. The main screens are usable at the largest text size in the agent's screenshots.
5. Reuben looks at the app on the phone, including a photo post, and agrees it works.

#### Result (2026-11-08)

All seven stages passed in one agent session (Claude Code, `claude -p --resume`), on Cati, with nobody touching the phone or the terminal during a stage. Phone time was about 19 minutes over the four phone stages.

| Stage | Time | Turns | What happened |
|---|---|---|---|
| `a-build` | 17.5 min | 98 | 14 Rust tests, 5 Swift model tests, iOS build passes. Fixed before the phone: a SQL parameter count (7 of 14 tests failed), a seed count, a Swift 6 Sendable warning, `Path`/`PathBuf` |
| `b-look` | 8 min | 70 | 3 installs. Found and fixed from screenshots and the tree: the "Save Password?" sheet covering the app after sign-up, like and reply targets 44x18 pt, unlabelled fields, an Unfollow button hard to see in dark mode |
| `c-test` | 5 min | 23 | Two failures, both real: the test's reset argument didn't apply to a running app (test fix, and Forge now stops the app first), and taps inside pushed screens did nothing (app fix). Journey 42/42 three times, largest text 19/19 |
| `change-1` | 2 min | 9 | Timeline spacing, one-line author row, larger action targets |
| `check-1` | 3.5 min | 20 | Fixed uneven spacing seen in a screenshot; both tests pass |
| `change-2` | 3 min | 26 | Bookmarks, with an in-place v1 to v2 database migration and its test |
| `check-2` | 2.5 min | 17 | The migration kept the existing data on the phone; journey 56/56 on the first run |

Forge fixes found by the run and made between stages: the client timeout (`MCP_TOOL_TIMEOUT`), `forge mcp --no-device`, `forge check swift` ignoring the app's toolchain pin, `launch` with arguments on a running app, and the UI test docs. Seen and left: SF Symbol images exposed with identifiers (`duplicate_identifier` warnings), tab buttons without identifiers, and `forge_check` reporting no test counts.

## Open decisions

1. **Agent sandbox bundle id.** Decided (Reuben, 2026-11-07): yes. Recommend `org.forgemobile.agentlab`, one slot, reused by every agent-made app until a person promotes it. The alternative (new id per app) burns slots and App IDs within a day.
2. **Subprocess now, library later.** Recommend subprocess with `--json` for M0. A library API comes when the JSON shapes have settled and the commands stop printing to stdout.
3. **Approval route.** Recommend elicitation plus a narrow `--allow` at startup. Some clients don't support elicitation yet; they get a refusal with the command a person can run.
4. **Ledger as data.** Decided (Reuben, 2026-11-07): one record per capability, with the Markdown tables generated from the records and checked in CI, because many parallel PRs edit the ledger. `forge://capabilities` reads the records.
5. **Is M0 the next thing after #61 and the dev-loop fixes?** Decided (Reuben, 2026-11-07): yes. The CLI items in M0's table are already on the dev-loop order. Adding item 7 costs 2–3 days on top.
6. **Paid Apple Developer Program?** Open. Reuben is on the free team (2026-11-07) and hasn't paid; the recommendation is to pay before M2, not M0 ([What a paid team would unlock](#what-a-paid-team-would-unlock)). `forge doctor`, `forge run ios` and `forge_device_status` report the team and its limits, so the tier and bundle-id rules can follow the paid column if he switches.

## Risks

- **Client support varies.** Structured content, elicitation and image results are in the spec but not in every client. Every result also has a text form, and every tier-2 refusal says what to run instead.
- **The agent's own judgement.** An agent can loop on a failure it can't fix (a signing error, a stuck USB). Results carry `needs_person`, the `fix_build` prompt says to stop on device and signing stages, and the server refuses to retry an install more than twice in a row without a change.
- **#61 hasn't passed a test on a phone yet.** On Cati (iOS 26.3.1, 2026-11-07) `idevice`'s XCTest start works: setup, launch, tree and screenshot all ran. Find by identifier (an attribute WDA 16 rejects) and keeping the runner alive after `forge` exits are open bugs. Screenshot through the developer screenshot service (DL3 fix 1) doesn't depend on WDA.
- **Liquid Glass and SDK stamping.** Linux builds may run in compatibility mode on iOS 26 ([ios-ui.md](ios-ui.md)). Screenshots an agent compares must come from the same kind of build.
- **Scope creep.** The point of M0 is one small app built and fixed unattended. Tools beyond the M0 list wait until a run shows the agent needs them.
