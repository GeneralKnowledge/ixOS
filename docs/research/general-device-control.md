# General device control: Forge operating apps it didn't build

Status: 2026-11-08. **Investigation and design only; nothing here is built.** The current milestone (Forge End-to-End Validation, then the Murmur social app) adds no large features, so this page is the plan for a separate workstream that starts after it. The phone questions it raises are queued as spikes for the next iPhone session ([Spikes](#g0-spikes-on-cati)), not answered here.

Reuben's proposal (2026-11-08): Forge already builds, installs and drives its own apps on a physical iPhone. The same machinery could launch, read and operate **any app installed on the phone**, which turns Forge from "an AI app-development environment" into "an AI-operated iPhone": building apps, and operating apps by other developers. Example tasks: review an app as a new user and report confusing flows and accessibility problems; build a playlist in a music app; summarise last month's transactions in a banking app; study an existing app as a reference, then build a native alternative with Forge.

## The short answer

- **Most of it already works, by accident of design.** The WebDriverAgent (WDA) route from [Forge UI Testing](../forge-ui-testing.md) is XCTest underneath, and XCTest is Apple's way to drive *any* app on the phone (`XCUIApplication(bundleIdentifier:)`). WDA's launch, activate, tree, tap, type, scroll and screenshot endpoints take any bundle id, and `forge ui tree` without `--app` already reads whatever app is in front. Apple's accessibility service ([ax-audit.md](ax-audit.md)) read the Claude app's identifiers on Cati with no runner at all.
- **What stops it today is deliberate: the guard.** Every `forge ui run` step and every MCP UI tool refuses to act unless the project's own app is in front (`UiError::WrongApp`, [Only the app under test](../forge-ui-testing.md#only-the-app-under-test)). General device control means replacing "only my app" with a policy a person writes, not removing the check.
- **Small missing pieces, no new automation stack:** listing installed apps with names (the USB `installation_proxy` service already does this, without the private API on-device tools need), launching Apple's own apps (today's lookup only knows user apps), swipe, Home, system alerts, open URL, a "back" that iOS doesn't have, a long-lived runner session (today every `forge ui` call starts and stops the runner), and locators for apps without identifiers.
- **The real work is safety.** Acting inside a banking or messaging app is a different risk class from tapping buttons in a Forge-built test app. The design puts an off-by-default switch, a person-written per-app policy, a gate on committing actions (send, pay, delete, post), hard refusals (passwords, payments, Settings) and untrusted-content handling into the server, where a tricked agent can't talk its way past them.
- **The free Apple team costs nothing extra here.** Third-party App Store apps don't use sideload slots; only the WDA runner does, and it is already installed for UI testing.
- **iCTRL** ([OpenMinis/iCTRL](https://github.com/OpenMinis/iCTRL)) is a well-built XCUITest runner aimed at exactly this use. It is the same kind of thing as WDA (an XCTest runner using private XCTest APIs), not a different route. Recommendation: don't adopt it now; borrow five of its ideas, and keep it as a candidate second `Driver` backend if WDA proves too slow on real-world apps ([evaluation](#ictrl)).

## Two use cases, one engine

| | Apps Forge builds | Apps already on the phone |
|---|---|---|
| Source and build | Forge owns both | None |
| Locators | Accessibility identifiers, by rule ([AGENTS.md](../../AGENTS.md) rule 4) | Whatever the app exposes: labels, element types, sometimes identifiers, sometimes nothing |
| Coordinates | Never | A last resort, recorded and policy-checked |
| Internal state | Devtools probes, logs, Rust state | Only what the screen shows |
| Who is at risk from a mistake | A test app with fixture data | The person's real accounts, messages and money |
| Safety model | [forge-mcp.md tiers](../forge-mcp.md#safety-boundaries): spends or loses something | This page: what the agent may do *inside* someone else's app |

The engine (WDA through `forge-uitest`'s `Driver` trait, over `idevice` and usbmuxd) is shared. What differs is locators and policy.

## What works today against apps Forge didn't build

From the code on `main` (e925e59) and the phone runs of 2026-11-07.

| Capability (Reuben's list) | Today | Evidence | Gap |
|---|---|---|---|
| `list_installed_apps` | Bundle ids of user apps (App Store and sideloaded): `Phone::user_apps()` via `installation_proxy` | Used by `forge ui setup` on Cati | No display names, no Apple system apps, not exposed as a command or tool |
| `launch_app` | `Driver::launch`/`activate` (`POST /wda/apps/launch`, `/activate`) take any bundle id | Pocket Library and Tipster on Cati | `resolve_app` only looks in user apps, so `com.apple.*` apps are refused; the MCP tool only launches the project's app |
| `get_foreground_app` | `Driver::foreground` (`GET /wda/activeAppInfo`) | Named the `XTL-` runner id on Cati | Not a tool of its own |
| `get_accessibility_tree` | `forge ui tree` reads whatever is in front | Pocket Library 2.3 s on Cati; third-party apps not yet run through WDA | Speed on long feeds unknown (spike P1) |
| (runner-free tree) | `ax_probe` element walk | **Read the Claude app's identifiers** (`BackButton`, `thread-checklist-pill`) on Cati, no runner, no DDI | Flat list, moves the VoiceOver focus visibly, experimental |
| `capture_screenshot` | `forge ui screenshot`, any app | 1290x2796 PNGs on Cati | None |
| `tap_element` | `Driver::tap` by identifier, label or label substring | Pocket Library, Tipster | Blocked by the front-app guard for other apps; apps without identifiers need better locators |
| (runner-free press) | `ax_probe --press` | Pressed `library.add` in a dev-signed app | Unknown on an App Store app, which lacks `get-task-allow` (spike P3) |
| `type_text` | `Driver::type_text` | Tipster, Pocket Library | Guard; secure fields need a policy ([Never](#safety-design)) |
| `scroll` | `Driver::scroll` (element or direction) | #92 | Guard |
| `swipe` | No | | WDA has `dragfromtoforduration` and element swipe |
| `press_back` | No; iOS has no system back | | Tap the navigation bar's first button, else a left-edge swipe |
| (home, alerts, open URL) | `background` only | | WDA has `/wda/homescreen`, `/alert/accept|dismiss`, `/url` |
| Navigate between apps | `activate` any bundle id; `com.apple.springboard` is the home screen | | Guard |
| App internal state | Devtools session, own apps only | | Not possible for other apps, by design |
| Change another app's code | No | | Out of scope |

**Apple's docs back this.** XCTest's `XCUIApplication(bundleIdentifier:)` is the public, documented way for one UI test to drive several installed apps, including Apple's. WDA is a long-running XCTest that exposes it over HTTP.

## What has to be added

All of it sits on the existing crates. No second device stack, no Appium, no new runner.

1. **App inventory.** `forge device apps [--all] [--json]`: bundle id, display name, version, and whether it is sideloaded, from `installation_proxy` with `ApplicationType` `User` and `System` and the `CFBundleDisplayName`/`CFBundleShortVersionString` attributes. From a USB host this is the official lockdown service, so Forge doesn't need the private `LSApplicationWorkspace` call that on-device tools like iCTRL rely on (and that iCTRL says newer iOS may refuse). `resolve_app` learns system apps and lookup by name ("Weather" → `com.apple.weather`).
2. **A long-lived runner session.** Today each `forge ui` command starts the runner through `testmanagerd` (about 3 s) and the runner dies when `forge` exits; each MCP UI call is a `forge --json` child, so it pays that every time. An operator loop makes hundreds of calls. Add `forge ui serve` (or keep the session inside `forge mcp`): one runner, one WDA connection, a device lock, reused across calls, and torn down on exit. This also fixes the open "runner dies when forge exits" bug for UI tests.
3. **Driver additions** in `forge-uitest`: `swipe(from, to, duration)`, `tap_point(x, y)` with a hit-test report (which element the point landed on, from the last tree's frames), `home()`, `alert(accept|dismiss|text)`, `open_url(url)`, `back()` (navigation-bar back button, else edge swipe). WDA has all of them; each is one request.
4. **Locators for apps without identifiers.** Keep `id`, `label`, `label_contains`. Add `kind` + `label` (a Button labelled "Library"), `kind` + `index` within a container, and `ref`: an opaque handle from the last `tree` result that is re-resolved at action time by path and label, and refused if the screen changed so it no longer matches. Coordinates only through `tap_point`, and only where policy allows them.
5. **Bounded reads.** Long feeds are where WDA is weakest (iCTRL measured this, see below). Use WDA's own settings (`snapshotMaxDepth`, `waitForIdleTimeout`, `animationCoolOffTimeout`) and return only visible elements by default, with `all: true` for the full tree.
6. **Policy and journal** ([Safety design](#safety-design)), in `forge-uitest` so the CLI and MCP share one implementation. The existing `ensure_front` becomes "act only on the app this session is bound to, and only if the policy allows that action there".
7. **MCP tools** ([Tool surface](#mcp-tool-surface)), listed only when the person starts the server with device control on.

What it doesn't need: changes to xtool, a paid Apple team, Wi-Fi control (USB first, as today), or a new crate. If the policy code grows, it can become a module `forge-uitest::control`.

## MCP tool surface

A separate group, `phone_*`, so an agent (and a person reading a transcript) can tell "operate the phone" from "build my app" (`forge_*`). Listed only with `forge mcp --device-control`.

| Tool | Reuben's name | Does | Kind |
|---|---|---|---|
| `phone_list_apps` | `list_installed_apps` | Installed apps with names and the policy level each has | Observe |
| `phone_foreground_app` | `get_foreground_app` | Bundle id and name of the app in front | Observe |
| `phone_inspect` | `inspect_screen` | Visible tree + screenshot in one call, with refs. The default way to look | Observe (policy-gated) |
| `phone_tree` | `get_accessibility_tree` | Full tree (`all: true` includes off-screen) | Observe (policy-gated) |
| `phone_screenshot` | `capture_screenshot` | PNG | Observe (policy-gated) |
| `phone_launch_app` | `launch_app` | Launch or activate by bundle id or name, and bind the session to it | Navigate |
| `phone_tap` | `tap_element` | By locator or ref; `x`/`y` only where allowed | Navigate or Commit |
| `phone_type` | `type_text` | Into a field by locator or ref | Navigate or Commit |
| `phone_scroll`, `phone_swipe` | `scroll`, `swipe` | Direction or element; from/to points | Navigate |
| `phone_back`, `phone_home` | `press_back` | Back in the bound app; Home screen | Navigate |
| `phone_alert` | | Read, accept or dismiss a system alert | Navigate or Commit (a permission alert is a commit) |
| `phone_open_url` | | Deep link or web URL | Navigate (target app's policy applies) |
| `phone_wait` | | Until an element appears or disappears | Observe |

Every result carries the app in front, so the agent notices when it has been thrown out of the app (a call, a notification tap, an app's own redirect). Every observe result is marked as app content: data, not instructions.

The existing project tools stay as they are. A project's own app needs none of this policy, so `forge_launch_app` and the `forge ui run` guard keep working unchanged.

## Safety design

The existing [tiers](../forge-mcp.md#safety-boundaries) are about what a call costs (slots, App IDs, account). Device control adds a second axis: **what the agent may do inside an app that holds the person's real life.** The server enforces it; MCP annotations are only hints, and an agent that read a malicious message in a chat app must not be able to talk its way past the rules.

**Off by default.** The `phone_*` tools don't exist unless the person starts `forge mcp --device-control`. The CLI equivalents (`forge ui ... --any-app`) need the same flag.

**A per-app policy the person writes,** in `~/.config/forge/device-control.toml` (not in a project, so an agent editing its project can't loosen it; writing it is tier 3, never through MCP):

```toml
default = "ask"                     # apps not listed: ask once per session

[apps]
"com.apple.weather" = "operate"     # read, navigate, type; commits still ask
"com.spotify.client" = "operate"
"com.example.bank" = "observe"      # read and scroll only
"org.forgemobile.agentlab" = "full" # agent-made apps: no commit gate
```

| Level | Allows |
|---|---|
| `deny` | Nothing. The server won't launch it, and refuses every call while it is in front |
| `observe` | Tree, screenshot, scroll, wait. No taps (a tap can be a commit), except `phone_back`/`phone_home` |
| `operate` | Taps, typing, swipes, alerts. **Commits need approval each time** |
| `full` | Everything except the hard refusals. For test apps and agent-made apps |
| `ask` | The first call in a session asks the person which level to use for this session |

**The commit gate.** A commit is an action that changes something outside the phone or can't be undone: send, post, reply, pay, buy, order, subscribe, transfer, delete, remove, unfollow, block, call, share, accept a permission alert. The server decides, not the agent: before a tap it reads the target's label and type from the tree and matches a commit list (localised later), and the agent can't skip the check by tapping coordinates, because `tap_point` hit-tests against the tree and gets the same check. A point that hits nothing identifiable is refused below `full`. An approved commit goes through MCP elicitation ("Tap 'Send' in Messages? Message text: …"); without elicitation the call is refused with the reason, as tier-2 calls are today. The list is a floor, not a guarantee: it will miss some, which is why money and messaging apps should default to `observe`.

**Never, at any level:**
- typing into a secure text field (passwords, passcodes, one-time codes), or reading one back;
- anything in Settings (`com.apple.Preferences`), Wallet, Passwords, the App Store, Find My, Phone and FaceTime calls, and the Apple ID screens: these are `deny` and can't be overridden in the file;
- Apple Pay or any payment sheet, Face ID and passcode prompts (which XCTest can't satisfy on a real phone anyway), deleting apps;
- CAPTCHAs and anti-bot checks (iCTRL refuses these too): the agent stops and asks the person.

So for "summarise last month's transactions", the person unlocks and signs in to the bank app; the agent only reads.

**Untrusted content.** Everything read from another app (tree text, OCR, screenshots) is someone else's content: a message, a post, a product listing. It is returned as data and never as instructions, and because the gates above are in the server, injected text can at worst make the agent *ask* for a commit.

**Privacy.** Screens of the person's apps are personal data, and [AGENTS.md](../../AGENTS.md) rule 5 says never log user content. Screenshots and trees go to a session folder outside any project (`~/.cache/forge/device-control/<session>/`), deleted at the end of the session unless the person keeps them, and never into Forge's command logs. Plainly: whatever the agent reads is sent to the agent's model provider, like any other tool result. The person should know that before pointing it at a banking app.

**A journal and a stop.** Every action is appended to the session journal: time, app, action, target label, and whether it was approved. Typed text is recorded as its length only. `forge ui stop` (and closing the MCP server) ends the runner at once. Locking the phone also stops everything, since nothing can act on a locked phone, and Forge never enters the passcode.

**Terms of service.** Some apps forbid automation. Forge is for the person's own phone and accounts, one task at a time; it does not create accounts, act in bulk or evade detection.

## Limits

| Limit | Effect | Mitigation |
|---|---|---|
| Free Apple team | The WDA runner uses one of 3 slots and is re-signed every 7 days. Third-party apps use no slots | Already the case for UI testing |
| Developer Disk Image, Enable UI Automation | Needed for WDA after every reboot; already documented | Runner-free reads through Apple's accessibility service ([ax-audit.md](ax-audit.md)) |
| USB only | The phone must be plugged into the laptop | Wi-Fi pairing is later work, as for the rest of Forge |
| Phone must be unlocked | No passcode entry, ever | Set Auto-Lock longer during a session |
| Thin accessibility trees | Some apps label badly or not at all | Labels and types, then refs, then screenshot + coordinates (policy permitting) |
| Custom-rendered UIs | Games (Metal, Unity) expose nothing; web views and Flutter expose part | The agent reads the screenshot (it is multimodal); OCR on Linux later if needed |
| Secure fields, biometrics, payments | Hidden values; Face ID and Apple Pay need the person | The person does those steps; policy refuses them anyway |
| Apps that detect automation | Synthesised touches look real to the system, but apps can use heuristics | No evasion. Report it and stop |
| Long feeds | WDA serialises off-screen rows, possibly seconds per read | Bounded, visible-only reads (spike P1 measures it) |
| XCTest attachments | iCTRL found XCTest's automatic screen recording piling up gigabytes of "System Data" after long runs | Check on Cati, whose storage is nearly full (spike P4) |
| Interruptions | Calls, notifications and alerts move another app to the front | Every result names the app in front; actions refuse on a different app |

## iCTRL

[OpenMinis/iCTRL](https://github.com/OpenMinis/iCTRL), read 2026-11-08 (README of the current release line, 1.9x). Apache-2.0 since 1.32.0 (AGPL before).

**What it is.** A UI test runner that never stops, like WDA, serving JSON-RPC over HTTP on port 8200. It is built for AI agents: `GET /skill` serves an operating manual and `GET /schema` the methods, so an agent given only an address teaches itself the API. Methods cover touch (tap, swipe, drag paths, long press), text, reading (tree, screenshot, on-device Vision OCR), apps (launch, activate, terminate, list, resolve by name, open URL) and device (home, lock, unlock). It also has a companion app that mounts the DDI on the phone itself, through LocalDevVPN and `idevice`, so it needs no computer at all.

**The private APIs.** Taps go through `XCSynthesizedEventRecord`/`XCPointerEventPath` (private XCTest) to skip XCTest's wait-for-idle; foreground and lock detection and early URL hooks use private APIs; `list_apps` uses `LSApplicationWorkspace`. That makes it unsuitable for the App Store, which doesn't matter for a sideloaded test runner. WDA also leans on private XCTest APIs, so this is not a difference in kind. The practical risk is that private APIs break across iOS releases, and both projects carry it.

**What it does better than WDA, by its own measurements:**
- **Speed:** taps about 280 ms median (no idle waits), visible-screen reads p50 416 ms across 8 apps, because snapshots are bounded in depth and breadth on the device. WDA took 0.6 s per tap and 2.3 s for a full tree on Cati with Pocket Library, and its reads grow with off-screen rows.
- **OCR fallback** when the tree is thin, with tappable coordinates.
- **Agent-shaped responses:** hints in every result, errors that say what to do next, re-resolving refs, look-alike warnings, scroll results that say whether anything moved.
- **Refuses** to query an app that left the screen, and refuses to touch anti-bot checks.
- **Security:** pairing approved on the phone, then X25519 + HMAC on every request. WDA is plain HTTP with no auth; Forge only reaches it through usbmuxd on USB, which limits the exposure.

**Why not adopt it now:**
- It is a second runner and a second protocol beside WDA, which [Forge UI Testing](../forge-ui-testing.md) just proved on Cati. On a free team it would take WDA's slot, so it would replace WDA rather than sit beside it.
- Installing it from Linux is unproven. It needs the nested `.xctest` signed from the inside out; xtool's `rcodesign` deep signing did that for WDA, so it may work, but `idevice`'s runner launch was only tested with WDA.
- Its design centre is an agent *on* the phone or on the LAN. Forge's is an agent on the laptop over USB, with the laptop holding the policy.

**What to borrow** (all fit the WDA route):
1. Bounded, visible-only snapshots by default.
2. Skip idle waits for taps where safe (WDA's `waitForIdleTimeout` setting).
3. A served manual: `forge mcp` already has resources and prompts; add a device-control skill page.
4. Hints and next steps in every error, plus look-alike warnings when a label matches several elements (#92 already ranks duplicates).
5. Refusing anti-bot checks and acting only on the bound app (Forge has the second).

**When to revisit:** if spike P1 shows WDA taking several seconds per read on real apps, try iCTRL's runner as a second `Driver` backend (`IctrlDriver`) on Cati. The `Driver` trait makes that a contained experiment.

## Milestones

None start before the current milestone (Murmur) is done.

### G0: spikes on Cati

No code and no new installs: the WDA runner and the apps already on Cati. Queued for the iPhone session plan. Each needs Reuben only to unlock the phone and pick a harmless app.

| Spike | Laptop runs | Proves |
|---|---|---|
| P1 | With a third-party App Store app in front (for example YouTube on its home feed) and then Apple's Weather: `forge --json ui tree` and `forge --json ui screenshot` | WDA reads other developers' apps on iOS 26.3.1; how many elements carry labels or identifiers; how long a long feed takes |
| P2 | `forge --json ui tree --app <that App Store bundle id>` and `--app com.apple.weather` | Activating another app works; the system-app lookup gap is real (expect the second to be refused today) |
| P3 | `ax_probe` walk and `--press` on a tab in the same App Store app | Whether the runner-free press works without `get-task-allow`. If yes, read-and-tap control needs no runner at all |
| P4 | Note Settings > General > iPhone Storage > System Data before and after the session | Whether WDA leaves XCTest recordings behind, which matters on a nearly full phone |

### G1: a read-and-navigate operator

App inventory, system-app lookup, the long-lived runner session, swipe, back, home, refs, bounded reads, the policy file with `observe`/`operate`/`deny`, the commit gate, the journal, and the `phone_*` tools. Mostly wiring around code that exists.

**Acceptance:** Reuben's third example, which is the lowest-risk and the closest to Forge's purpose. "Go through an app as a new user and report confusing interactions, accessibility problems and broken flows," on an app Reuben picks and marks `operate`, with no commits approved. The report's claims link to screenshots and tree excerpts from the session.

### G2: typing, alerts and approved commits

Typing, system alerts, open URL, elicitation for commits.

**Acceptance:** a playlist-style task in an app Reuben allows: create something, approve one commit, and the journal shows exactly what happened.

### G3: reference-to-native loop

An agent inspects an existing app with G1, writes a short spec (screens, flows, accessibility notes), builds a Forge app from it, and compares the two side by side with the same tools. This joins the two halves of Forge and is the strongest demonstration of the idea.

## Open decisions

1. **Is general device control a workstream after Murmur?** Recommended: yes, starting with G0's spikes in the next phone session, since they cost nothing.
2. **The default level for unlisted apps:** `ask` (recommended) or `deny`.
3. **Commit approval without elicitation:** refuse with a message (recommended, as tier 2 does today), or allow a per-session standing allowance per app.
4. **Banking and messaging apps:** `observe` only for now (recommended), revisited after G2.

## Sources

- Code: `crates/forge-uitest/src/{lib,wda,phone}.rs`, `crates/forge-cli/src/ui.rs`, `crates/forge-mcp/src/tools.rs`, `crates/forge-device/examples/ax_probe.rs` at e925e59.
- Phone results: [forge-ui-testing.md](../forge-ui-testing.md#checked-on-the-phone), [ax-audit.md](ax-audit.md#results-so-far).
- iCTRL README: <https://raw.githubusercontent.com/OpenMinis/iCTRL/HEAD/README.md>, release v1.96.1.
- Apple: [`XCUIApplication`](https://developer.apple.com/documentation/xctest/xcuiapplication) (`init(bundleIdentifier:)`).
