# Any app on the iPhone: growing Forge UI Testing beyond Forge's own apps

Status: design, 2026-11-08, revised the same day after Reuben's review, with the first phone results ([Spikes on Cati](#spikes-on-cati), 2026-11-08). **Built so far: priority 1**, the policy, commit gate, journal and `--any-app` for `forge ui` ([Apps Forge didn't build](../forge-ui-testing.md#apps-forge-didnt-build); tested on Linux, not yet on the phone). The rest is design.

**What this is not:** a separate phone-control workstream or a second automation stack. Forge already has the architecture: [Forge UI Testing](../forge-ui-testing.md) (`forge-uitest`'s `Driver`, `UiNode` and `Locator`), and the [agent capability audit](forge-agent-capability-audit.md) of 2026-11-07 already proposed the missing routes: A2 (Apple's accessibility service), A3 (the rest of `idevice`), the exploratory crawler, and evidence bundles (A4). This page applies those plans to apps Forge didn't build, and makes "any app" a first-class use of the same system.

**What Forge can't claim yet:** that it tests arbitrary third-party apps end to end. Tipster proved the agent loop on a physical iPhone for an app Forge built. On another developer's app, Forge has so far read the screen and, once, typed and sent one message with Reuben watching ([spikes](#spikes-on-cati)); that is a working route, not a tested app. Every route below still needs its own validation on other developers' apps. And controlling the screen does not give access to another app's private data: its container, logs and state stay out of reach, and only what the screen shows is readable.

## The idea

Reuben's proposal (2026-11-08): the machinery that builds, installs and drives Forge's own apps could also launch, read and operate any app installed on the phone. Example tasks: review an app as a new user and report confusing flows and accessibility problems; build a playlist in a music app; summarise last month's transactions in a banking app; study an existing app as a reference, then build a native alternative with Forge.

## Three levels

| Level | What the agent can do | Where Forge is |
|---|---|---|
| **Observe** | Read the screen's accessibility structure, labels and values; screenshots; which app is in front; performance and operating conditions | WDA reads any foreground app today; Apple's accessibility service read another developer's app (the Claude app) on Cati with no runner. A3's performance services aren't wired yet |
| **Operate** | Tap, scroll, swipe, type, navigate between apps, handle alerts | Works for Forge's own apps (Pocket Library, Tipster on Cati). **Worked once in an App Store app** (2026-11-08: typed into the Claude app's `chat-input` and tapped `Send`, and the message arrived). Missing a few calls and a policy (below) |
| **Understand and verify** | Explore workflows, recognise failures and dead ends, decide whether an app behaves correctly, and show the evidence | Not built. The crawler (UI Testing M5) and evidence bundles ([evidence.md](../evidence.md)) are the pieces. **The biggest opportunity** |

## One device model, several routes

The audit's architectural rule ([section 8](forge-agent-capability-audit.md#8-architectural-implications)) is the design: `forge-device` owns the phone (selection, lock, DDI, tunnel, services); `forge-uitest` owns the UI through one `Driver` trait, one `UiNode` model and one `Locator` vocabulary. Every route feeds those, and Forge picks the route per operation.

| Route | Role | Status |
|---|---|---|
| **WebDriverAgent** (XCTest) | **The primary interaction route.** The only one that types, launches, handles system alerts and reads the full tree in one call | Proven on Cati for Forge's apps (M0 11/11, Tipster) |
| **Apple's accessibility service** (A2, `axAuditDaemon`) | **An additional driver** (`AxDriver`, UI Testing M2.5): reading and pressing with no runner, no app slot and no DDI; Apple's own audit; accessibility settings such as text size | Probe reads and presses on Cati ([ax-audit.md](ax-audit.md)); audit not answering yet; press on an App Store app untested |
| **CoreDevice HID** (`idevice`) | Later: raw touches and buttons without a runner, for screens the tree can't describe | Not tried |
| **A3 device services** (`idevice`) | Conditions and measurements around whatever app is in front | In the pinned `idevice`; not exposed |

`forge ui` and the MCP UI tools stay the interface. Adding an app that Forge didn't build changes which app the session is bound to and which policy applies, not which commands exist.

## What works today against other developers' apps

From the code on `main` and the phone runs of 2026-11-07.

| Capability | Today | Gap |
|---|---|---|
| List installed apps | `Phone::user_apps()` via `installation_proxy` (bundle ids of App Store and sideloaded apps) | No display names, no Apple system apps, not a command or tool |
| Launch or switch to an app | `Driver::launch`/`activate` (WDA) take any bundle id; `forge ui tree --app com.anthropic.claude` was accepted on Cati (P2) | `resolve_app` only knows user apps, so `com.apple.*` is expected to be refused (untested: Weather isn't on Cati); the MCP tool only launches the project's app |
| Which app is in front | `Driver::foreground` (`/wda/activeAppInfo`) | Not a tool of its own |
| Accessibility tree | `forge ui tree` reads whatever is in front: **the Claude app on Cati gave 160 elements, 38 labelled, 15 with identifiers (`chat-input`, `chat-model-picker`, `Send`, `Stop`, `BackButton`…) in 4 s** (P1) | 4 s includes starting the runner; a long feed is still unmeasured |
| Runner-free reading (A2) | `ax_probe` read the Claude app: 20 elements in VoiceOver order, all labelled, 5 with identifiers (P3) | Experimental; flat list; moves the VoiceOver focus visibly |
| Screenshot | `forge ui screenshot`, any app | None |
| Tap, type, scroll | `Driver` by identifier, label or label substring. **`forge ui run` with a test file naming the Claude app typed into `chat-input` and tapped `Send` on Cati**; about 10 s for the run, mostly about 2 s of runner start-up per step | Policy built since (`--any-app`, levels, commit gate; see below). Apps without identifiers need better locators |
| Runner-free press (A2) | `ax_probe --press` in a dev-signed app | Still unknown on an app without `get-task-allow`: P3 walked but didn't press |
| Swipe, back, Home, alerts, open URL | No (`background` only) | WDA has each; iOS has no system back, so back means the navigation bar's button or an edge swipe |
| Another app's internal state | Devtools, own apps only | Not possible, by design |

`XCUIApplication(bundleIdentifier:)` is Apple's documented way for one UI test to drive several installed apps, Apple's included; WDA is a long-running XCTest that exposes it over HTTP. Forge's guard (`UiError::WrongApp`, [Only the app under test](../forge-ui-testing.md#only-the-app-under-test)) only checks that the app *named in the test file* is in front, and a test file can name any user-installed app. **So the CLI could act inside any App Store app, with no policy at all**, as the Claude app run showed; only the MCP server limited itself to the project's app. That made the policy below the first thing to build, and it is now built: another app needs `--any-app` and a level from the person's policy file, and commits ask ([Apps Forge didn't build](../forge-ui-testing.md#apps-forge-didnt-build)). The check that the named app is in front stays.

## A3 services, and which are app-independent

The audit's A3 table, read for apps Forge didn't build. These are app-independent capabilities, exposed selectively as small `forge device` commands on `forge-uitest`'s `Phone` code.

| Command (A3) | `idevice` service | Any app? |
|---|---|---|
| `forge device network <profile>` | DVT `condition_inducer` | **Yes**: phone-wide. Offline and poor-network behaviour of any app |
| `forge device location` | DVT `location_simulation` | **Yes**: phone-wide |
| `forge device perf --app` | `graphics` (FPS), `sysmontap` (CPU, memory per process) | **Yes**: by process. "Does this app stutter while scrolling?" |
| `forge device rotate` | CoreDevice `orientation_service` | **Yes** |
| `forge device pasteboard` | CoreDevice `pasteboard_service` | **Yes**, and sensitive: it reads what the person copied. Write only by default |
| Process list and kill | DVT `process_control`, `deviceinfo` | **Yes**: also the invisible "which app is in front" check |
| `forge device container` | `house_arrest` | **No**: only apps signed with `get-task-allow` (development builds). Other apps' data stays private |
| Crash reports | `crashreportcopymobile` (already in `forge device crashes`) | Yes for the reports iOS writes; not symbolicated |

## What has to be added

All on existing crates; no new runner, no Appium, no new device stack.

1. **App inventory.** `forge device apps [--all]`: bundle id, display name, version, sideloaded or not, from `installation_proxy` (`User` and `System`). From a USB host this is the official lockdown service, so Forge doesn't need the private `LSApplicationWorkspace` call on-device tools rely on. `resolve_app` learns system apps and names ("Weather" → `com.apple.weather`).
2. **A long-lived runner session.** Each `forge ui` command now starts the runner (about 2 s per step measured on Cati on 2026-11-08, most of a 10 s two-step run) and the runner dies when `forge` exits; each MCP UI call is a child `forge` process that pays this again. An exploring agent makes hundreds of calls. One session (`forge ui serve`, or held by `forge mcp`) keeps one runner, one WDA connection and the device lock. This also fixes the open "runner dies when forge exits" bug for UI tests.
3. **`Driver` additions**: `swipe`, `tap_point` with a hit test against the last tree (which element the point landed on), `home`, `alert`, `open_url`, `back`. One WDA request each.
4. **Locators for apps without identifiers.** Keep `id`, `label`, `label_contains`. Add `kind` + `label`, `kind` + `index` in a container, and `ref`: a handle from the last tree, re-resolved at action time and refused if the screen no longer matches. Coordinates only through `tap_point`, and only where policy allows. [AGENTS.md](../../AGENTS.md) rule 4 (never coordinates) stays for Forge's own apps and tests.
5. **Bounded reads.** Visible elements only by default, using WDA's `snapshotMaxDepth`, `waitForIdleTimeout` and `animationCoolOffTimeout` settings.
6. **`AxDriver`** (UI Testing M2.5, A2) behind `Driver`, chosen per operation: reads and presses without the runner, WDA for the rest.
7. **Policy and journal** (below), in `forge-uitest` so CLI and MCP share it. `ensure_front` becomes "act only on the app this session is bound to, and only if the policy allows that action there".
8. **MCP.** The existing UI tools grow an app argument instead of a new tool group: `forge_launch_app` takes a bundle id or name; tree, screenshot, tap, type and scroll act on the bound app; `forge_device_apps`, `forge_foreground_app`, swipe, back, home, alert and open-URL are added. Apps outside the project are reachable only when the person starts `forge mcp --any-app`. Every result names the app in front and marks screen content as app output.

## The crawler, on the unified UI model

UI Testing's exploratory tester ([M5](../forge-ui-testing.md#milestones)) and the audit's crawler are the same thing, and it is the "understand and verify" level. It walks `UiNode` trees from whichever route is cheapest, so it works the same on any app:

- **Visit** reachable screens: enabled buttons, cells, tabs, back; a screen is identified by its structure (kinds, labels, identifiers), not its pixels.
- **Check** each screen: Apple's audit (A2) or Forge's tree audit, truncated text at the largest Dynamic Type size, unlabelled controls, dead ends (no way back), alerts with errors, crash reports after each action, and A3 FPS and memory while scrolling.
- **Stay safe in other apps:** it obeys the same policy as an agent; it never taps a commit (send, pay, delete, post) and never types into secure fields. In other developers' apps it runs at the observe or operate level with commits off, which matches what a new-user review needs.
- **Write down what it found** as replayable steps: a normal `forge ui run` file for Forge's own apps; an evidence bundle for any app.

First on Forge's own apps (Pocket Library, Murmur), where it can be checked against the source, then on an app Reuben picks.

**Update, 2026-11-08:** the first version is `forge ui explore` ([forge-explore.md](../forge-explore.md)), for Forge apps only until the policy above lands. It is built and tested on Linux; not yet run on an iPhone.

## Evidence: what the agent actually inspected and tested

[evidence.md](../evidence.md) already defines `forge.evidence/1` and proposes device runs for `forge ui run`. Extend it rather than invent a format: an exploration or an any-app task writes a bundle with every step (action, target, route used, app in front, result), the trees and screenshots it looked at, audit findings, A3 measurements and crash reports. A report's claims ("the Save button has no label") link to the step and screenshot that show it, so Reuben can check a claim in a minute. For other developers' apps the bundle is personal data: it stays in a local folder outside any project and is never committed ([Privacy](#safety)).

## Safety

The [MCP tiers](../forge-mcp.md#safety-boundaries) are about what a call costs (slots, App IDs, the account). Other people's apps add a second axis: what the agent may do inside an app that holds the person's accounts, messages and money. The server enforces it, so an agent that read a malicious message in a chat app can't talk its way past the rules.

**Off by default.** Apps outside the project are unreachable unless the person starts `forge mcp --any-app` (or passes `--any-app` to `forge ui`).

**A per-app policy the person writes,** in `~/.config/forge/device-control.toml`: outside any project, so an agent editing its project can't loosen it; writing it is tier 3, never through MCP.

```toml
default = "ask"                     # apps not listed: ask once per session

[apps]
"com.apple.weather" = "operate"     # read, navigate, type; commits still ask
"com.example.bank" = "observe"      # read and scroll only
"org.forgemobile.agentlab" = "full" # agent-made apps: no commit gate
```

| Level | Allows |
|---|---|
| `deny` | Nothing. Not launched, and every call is refused while it is in front |
| `observe` | Tree, screenshot, scroll, wait, back, Home. No taps |
| `operate` | Taps, typing, swipes, alerts. **Commits need approval each time** |
| `full` | Everything except the hard refusals. For Forge's own and agent-made apps |
| `ask` | The first call in a session asks the person which level applies |

**The commit gate.** A commit changes something outside the phone or can't be undone: send, post, reply, pay, buy, order, subscribe, transfer, delete, remove, block, call, share, grant a permission. Before a tap the server reads the target's label and kind from the tree and matches a commit list; a coordinate tap is hit-tested and gets the same check, and one that hits nothing identifiable is refused below `full`. Approval is by MCP elicitation, or the call is refused with the reason, as tier-2 calls are today. The list will miss some, which is why money and messaging apps default to `observe`.

**Never, at any level:** typing into or reading secure fields (passwords, passcodes, one-time codes); Settings, Wallet, Passwords, the App Store, Find My, Phone and FaceTime, and the Apple ID screens; Apple Pay and payment sheets, Face ID and passcode prompts; deleting apps; CAPTCHAs and anti-bot checks (the agent stops and asks). For the banking example, the person signs in and the agent only reads.

**Untrusted content.** Text and screenshots from other apps are someone else's content. They are returned as data, never instructions; because the gates are in the server, injected text can at worst make the agent ask for a commit.

**Privacy.** Screens of the person's apps are personal data, and AGENTS.md rule 5 says never log user content. Trees, screenshots and bundles go to `~/.cache/forge/any-app/<session>/`, are deleted at the end of the session unless the person keeps them, and never enter Forge's command logs. Apple's accessibility service adds one more: while its app monitoring is on, it reports every app on the phone moving to the foreground or background (seen on Cati, P3), so an `AxDriver` must switch monitoring on only for the walk, off straight after (as `ax_probe --off` does), and drop events about other apps. What the agent reads is also sent to its model provider, like any tool result; the person should know that before pointing it at a banking app.

**Journal and stop.** Every action is journalled (time, app, action, target label, approved or not; typed text as its length only). `forge ui stop` or closing the MCP server ends the runner at once. A locked phone stops everything; Forge never enters the passcode.

**Terms of service.** Some apps forbid automation. This is for the person's own phone and accounts, one task at a time: no account creation, no bulk actions, no evading detection.

## Limits

| Limit | Effect | Mitigation |
|---|---|---|
| Free Apple team | The WDA runner uses one of 3 slots and is re-signed every 7 days. Other developers' apps use no slots | Already the case for UI testing; `AxDriver` reads without the runner |
| DDI, Enable UI Automation | Needed for WDA after every reboot | Documented in forge-ui-testing.md |
| USB only | The phone is plugged into the laptop | Wi-Fi is later work for all of Forge |
| Phone must be unlocked | No passcode entry, ever | Longer Auto-Lock during a session |
| Thin accessibility trees | Some apps label badly or not at all | Labels and kinds, refs, then screenshot and coordinates where policy allows |
| Custom-rendered UIs | Games expose nothing; web views and Flutter expose part | The agent reads the screenshot; OCR on Linux later if needed |
| Secure fields, biometrics, payments | Hidden values; Face ID and Apple Pay need the person | The person does those steps; policy refuses them anyway |
| Automation detection | Apps can use heuristics | No evasion: report and stop |
| Long feeds | WDA serialises off-screen rows, possibly seconds per read. A chat screen took 4 s including runner start-up | Bounded reads; measure a real feed once the long-lived session removes start-up from the figure |
| XCTest attachments | iCTRL found XCTest recordings piling up as "System Data" after long runs | Check on Cati, whose storage is nearly full (spike P4, not done yet) |
| Interruptions | Calls and notifications change the app in front | Every result names the app in front; actions refuse on another app |

## iCTRL

[OpenMinis/iCTRL](https://github.com/OpenMinis/iCTRL), read 2026-11-08 (README, release line 1.9x). Apache-2.0 since 1.32.0 (AGPL before).

**What it is.** An XCUITest runner that never stops, like WDA, serving JSON-RPC over HTTP on port 8200, built for AI agents: `GET /skill` serves an operating manual and `GET /schema` the methods. It covers touch, text, reading (tree, screenshot, on-device Vision OCR), apps (launch, list, resolve by name, open URL) and device (home, lock). A companion app mounts the DDI on the phone itself.

**Private APIs.** Taps use private XCTest event synthesis (`XCSynthesizedEventRecord`, `XCPointerEventPath`) to skip XCTest's idle waits; foreground and lock detection use private APIs; `list_apps` uses `LSApplicationWorkspace`. WDA also relies on private XCTest APIs, so this is not a difference in kind. Both carry the risk that an iOS release breaks them.

**Better than WDA, by its own measurements:** taps about 280 ms (WDA took 0.6 s on Cati), visible-screen reads p50 416 ms across 8 apps through bounded snapshots, OCR when the tree is thin, hints and next steps in every response, refusing apps that left the screen and anti-bot checks, and pairing approved on the phone with encrypted requests (WDA is plain HTTP; Forge reaches it only over USB).

**Not adopted:** it would be a second runner beside WDA, which is proven on Cati, and on a free team it would take WDA's slot. Installing it from Linux (inside-out signing of its `.xctest`, launch through `idevice`) is unproven. Its design centre is an agent on the phone or the LAN; Forge's is an agent on the laptop holding the policy.

**Borrowed:** bounded visible-only reads; skipping idle waits where safe; a served operating manual (an MCP resource); hints and look-alike warnings in errors; refusing anti-bot checks. If spike P1 shows WDA taking seconds per read on real apps, iCTRL's runner is a candidate `Driver` backend, which the trait keeps contained.

## Priorities

Updated from the audit's order now that Tipster has proved the agent loop on a phone. None start before Murmur is done, except the spikes, which cost no code.

| | Work | Builds on | Proves |
|---|---|---|---|
| 0 | Spikes P1–P4 on Cati (below) | Today's code | **Done 2026-11-08**, except P4, a runner-free press and an Apple app: WDA reads and operates an App Store app |
| 1 | Policy, commit gate, journal, `--any-app`. **Built** for `forge ui` (Linux tests; phone run pending). The MCP `--any-app` waits for priority 2 | `ensure_front` | Safe operation outside the project |
| 2 | Driver seam: long-lived session, `AxDriver` (A2), locators without identifiers, swipe/back/home/alert/open URL, app inventory | UI Testing M2.5, `Phone` | Observe and operate on any app, route chosen per operation |
| 3 | A3 commands, selectively: network, location, perf, rotate, process list | `Phone`, audit A3 | Conditions and measurements around any app |
| 4 | Crawler | 1–3, UI Testing M5 | Understand and verify, first on Forge's apps |
| 5 | Evidence bundles for device runs and explorations | evidence.md, `forge.evidence/1` | What the agent actually inspected and tested |

**Acceptance, in order:** (a) the crawler finds real issues in Pocket Library that a person confirms; (b) a new-user review of an app Reuben picks, at the `operate` level with no commits approved, every claim linked to evidence; (c) an agent studies an existing app with these tools, writes a short spec, builds a Forge alternative and compares the two with the same crawler. (c) joins the two halves of Forge.

### Spikes on Cati

Run on 2026-11-08 in the iPhone session, with no code changes and no new installs: forge at c03549d, the WDA runner already on Cati, and the Claude app (`com.anthropic.claude`, from the App Store) as the third-party app, with Reuben watching. Evidence (JSON, trees, screenshots) is on the laptop in `~/forge-uitest/g0/`.

| Spike | Ran | Result |
|---|---|---|
| P1 | `forge --json ui tree` and `ui screenshot` with the Claude app in front | **Works.** 160 elements, 38 labelled, 15 identifiers (`chat-input`, `chat-model-picker`, `Send`, `Stop`, `BackButton`…), 4 s including runner start-up. Screenshot fine. Apple's Weather isn't installed on Cati, so no Apple-app result |
| P2 | `forge --json ui tree --app com.anthropic.claude` | **Works**: an App Store app is accepted and activated. The system-app case (`com.apple.*`) is still to try with an Apple app that is installed |
| P3 | `ax_probe` walk on the Claude app, then `--off` | **Walk works**: 20 elements, all labelled, 5 identifiers. The service also reported every app's foreground and background changes while monitoring was on (a privacy point, see [Safety](#safety)). `--off` cleaned up. **No press tried**, so the `get-task-allow` question stays open |
| Extra, at Reuben's request | A `forge ui run` file naming the Claude app: type into `chat-input`, tap `Send` once | **Works.** The message "Hello from Forge on Linux: typed and sent by WebDriverAgent over USB, no app installed." arrived in the conversation, sent as Reuben, at 09:59. About 10 s of forge time, mostly runner start-up (about 2 s per step). `Send` is replaced by `Stop` while the assistant answers, so a test must wait for `Send` to come back |
| P4 | System Data before and after | Not done |

What they settle: WDA reads and operates another developer's app on iOS 26.3.1 with the free team and nothing extra installed, and that app exposes enough identifiers and labels to find its main controls without coordinates. What they show needs doing first: today's CLI has no policy for other apps, the per-call runner start-up dominates the time, and A2 monitoring leaks other apps' activity unless switched off. Still open: a runner-free press on an App Store app, an Apple system app, a long feed, and System Data.

## Open decisions

1. **Default for apps not in the policy file:** built as `ask`, which with no terminal is a refusal. A file can say `default = "deny"`.
2. **Commit approval without elicitation:** built as refuse with a message. A per-session allowance per app is not built.
3. **Banking and messaging apps:** `observe` only until the crawler and policy have run on harmless apps (recommended).

## Sources

- Code: `crates/forge-uitest/src/{lib,wda,phone}.rs`, `crates/forge-cli/src/ui.rs`, `crates/forge-mcp/src/tools.rs`, `crates/forge-device/examples/ax_probe.rs`.
- [Agent capability audit](forge-agent-capability-audit.md): A2, A3, A4, the crawler (7.3), section 8. [Forge UI Testing](../forge-ui-testing.md): routes, M2.5, M5, phone results. [ax-audit.md](ax-audit.md). [evidence.md](../evidence.md).
- iCTRL README: <https://raw.githubusercontent.com/OpenMinis/iCTRL/HEAD/README.md>, release v1.96.1.
- Apple: [`XCUIApplication`](https://developer.apple.com/documentation/xctest/xcuiapplication) (`init(bundleIdentifier:)`).
