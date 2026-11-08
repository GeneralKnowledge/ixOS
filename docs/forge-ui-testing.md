# Forge UI Testing: driving a real iPhone from Linux

Status: 2026-11-07. **M0 passed on a physical iPhone**: Cati (iPhone 15 Pro Max, iOS 26.3.1, free Apple team), WebDriverAgent 16.15.2, `forge --json ui run examples/pocket-library/uitests/m0.toml` with 11 of 11 steps in 10 s, two 1290x2796 screenshots, run from Linux over USB and watched by Reuben. The phone needed Settings > Developer > Enable UI Automation turned on. Step times: launch 0.1 s, screenshot 0.3 s, taps 0.6 s, typing 1.8 s, expectations 0.5–0.7 s, tree 2.3 s. Later milestones are plans.

Forge UI Testing lets a Linux machine drive an ixOS app on a physical iPhone the way a person would: launch it, find controls by their accessibility identifier, tap, type, read what is on screen, and take screenshots. It is a framework capability, like [Forge Device](forge-device.md), so every demo (Chat, Pocket Library, Weather and the ones that follow) gets it without building its own test harness.

It has two goals, and they are deliberately the same piece of work:

1. **Robust tests.** Tests find elements through the accessibility tree (identifiers, labels, traits, values), never by screen coordinates, so they survive layout changes.
2. **Better accessibility for real users.** The same tree is what VoiceOver, Voice Control, Switch Control and Dynamic Type users rely on. Every identifier and label a test needs is also a label a VoiceOver user hears, and the tree the tests read can be audited for missing labels and truncated text.

## The short answer

It works from Linux with a free Apple account, and it can go through the connection Forge Device already uses. No Mac, no Xcode install, no root and no Appium server are needed.

- **Apple's own UI automation (XCTest / XCUIAutomation) is the engine.** Only XCTest can query the accessibility tree of another app and synthesise touches into it, including system alerts. Everything else (WebDriverAgent, Appium, go-ios's `runwda`) is a wrapper around it.
- **WebDriverAgent (WDA) is the on-phone half.** Appium publishes an unsigned real-device build of the WDA test runner. `xtool install` re-signs it with your Apple ID (free accounts included) and installs it. We don't have to compile WDA on Linux.
- **The `idevice` crate that Forge Device already uses can run it.** `idevice` 0.1.68 (the exact version `forge-device` pins) has an `xctest` feature that starts an XCTest runner through `testmanagerd`. It has a `wda` feature with a WebDriverAgent client over usbmuxd. On iOS 17 and later it reaches `testmanagerd` through the `CoreDeviceProxy` lockdown service and a userspace TCP stack, so no TUN device and no root are needed. Forge UI Testing is a new crate on top of the same device connection, not a second device stack.
- **Two costs are specific to iPhone development without a Mac.**
  - The runner is a separate app, so it uses **one of the three free-account app slots**, and it expires with the other apps after **7 days**.
  - XCTest needs the **Developer Disk Image (DDI)** mounted. On iOS 17 and later it is a personalised image that Apple signs per device, and it is unmounted at every reboot. Xcode mounts it for Mac users. We mount it from Linux with `idevice`. The image does not come with xtool's SDK. The Xcode 26.0 `Xcode.xip` we checked on Cati has no `CoreDeviceDDIs` folder at all, so on 2026-11-07 the image came from the [doronz88/DeveloperDiskImage](https://github.com/doronz88/DeveloperDiskImage) mirror (`PersonalizedImages/Xcode_iOS_DDI_Personalized`, at commit e2aaeac). Apple still signs the image for each phone when it is mounted.

The rest of this page is the evidence for each claim, the design, and the milestones.

## The routes we checked

### 1. Native XCTest / XCUIAutomation

This is Apple's UI testing framework. A UI test bundle (`.xctest`) runs inside a runner app (`<Name>-Runner.app`). The runner talks to `testmanagerd`, which proxies accessibility queries and synthesised events into the app under test. `XCUIElement` queries match on accessibility identifier, label, value and element type.

- **It works on Linux only if something other than Xcode starts the run.** `xcodebuild test` is macOS-only. The protocol underneath is DTX messages to `testmanagerd` and the instruments `processcontrol` service. It is undocumented, but go-ios, pymobiledevice3, tidevice and now `idevice` implement it.
- **Writing our own test bundles in Swift is not practical yet.** xtool builds apps and app extensions, not `.xctest` bundles. A UI test bundle links `XCTest.framework` from the iPhoneOS platform directory and is wrapped in Apple's `XCTRunner` agent. xtool's SDK does keep `Platforms/iPhoneOS.platform/Developer/Library/Frameworks` (so `XCTest.framework` is there), but not the `XCTRunner` agent, and there's no SwiftPM product type for it. That is a later option (M5), not the start.
- **Verdict: use XCTest as the engine, through a prebuilt runner (WDA), rather than writing per-app XCTest bundles.** Tests are then written once, in Forge's own format, and run against any app.

### 2. WebDriverAgent, signed with xtool, without Xcode

WDA is an XCTest UI test bundle that never finishes: it starts an HTTP server on the phone (port 8100) and turns WebDriver requests into XCUIAutomation calls. It finds elements by `accessibility id` (WDA's `name` attribute: the identifier if set, otherwise the label), class chain or predicate. It taps, types, swipes, launches and backgrounds apps, presses the Home button, accepts alerts, returns the whole UI tree as XML, and takes PNG screenshots.

What we checked:

- **Prebuilt runner.** `WebDriverAgentRunner-Runner.zip` from Appium's WDA release v16.15.3 (16 Oct 2026; 1,145,983 bytes, sha256 `2176793538d902f6b1e61291e9997f68d306541f222ea715d256a7c6ce9d4fc5`). It contains `WebDriverAgentRunner-Runner.app` with `PlugIns/WebDriverAgentRunner.xctest` and `WebDriverAgentLib.framework`. It is **unsigned** (no `_CodeSignature`), its bundle id is `com.facebook.WebDriverAgentRunner.xctrunner`, and it has **no embedded `XC*` frameworks**. On iOS 17 and later XCTest is loaded from the mounted DDI, which is why the DDI is required. The WDA repository was archived on 17 Oct 2026; Appium now publishes later versions from `appium/appium-ios` under `appium-webdriveragent@<version>` tags. We pin one version and its checksum.
- **Signing with xtool.** `xtool install <path>` takes "a custom app/ipa", signs it with the logged-in Apple ID and installs it (`Sources/XToolSupport/InstallCommand.swift`). Signing is done by `rcodesign` with deep signing, so the nested `.xctest` and framework are signed too (`Sources/XKit/Signer/RCodesignSigner.swift`). xtool registers the App ID as `XTL-<id>.<bundle id>` (`Sources/XKit/Utilities/ProvisioningIdentifiers.swift`), so the installed runner's bundle id is `XTL-…​.com.facebook.WebDriverAgentRunner.xctrunner`. Forge looks it up by suffix rather than hard-coding it. Its executable still ends in `-Runner`, which is what the runner launch requires.
- **Starting it without Xcode.** `idevice::services::dvt::xctest::XCUITestService::run_until_wda_ready` builds the `XCTestConfiguration`, opens `testmanagerd`, launches the runner through instruments, and waits until WDA answers `/status`. The test session lives on a background task for as long as we hold it.
- **Verdict: yes, this is the M0 route.** Not yet run on an iPhone.

### 3. Appium on a Linux host

Appium's XCUITest driver now documents [limited support for non-macOS hosts](https://appium.github.io/appium-xcuitest-driver/latest/guides/non-macos-hosts/): real devices only, **iOS 18 or later**, `appium:udid` and `appium:platformVersion` required, and the usual `xcodebuild` WDA startup is "not supported". WDA must already be on the phone (`usePreinstalledWDA`) or running elsewhere (`webDriverAgentUrl`). It needs a [RemoteXPC tunnel](https://appium.github.io/appium-xcuitest-driver/latest/guides/remotexpc-tunnels-real-devices/) created with `sudo appium driver run xcuitest tunnel-creation`, which "must be run as sudo/root to create TUN/TAP interfaces". Those docs also say the fallback without a tunnel is "temporary, limited and expected to stop working".

- What it would add: the WebDriver ecosystem (clients in every language, Appium Inspector).
- What it would cost: Node.js and an Appium server, a root tunnel process, and still a WDA we sign and install ourselves. The parts that are hard on Linux (signing, installing, mounting the DDI, starting the runner) are exactly the parts Appium leaves to us.
- **Verdict: don't depend on it.** Because the on-phone half is plain WDA, an Appium client can still attach to a runner Forge started (`webDriverAgentUrl`) if someone wants Appium Inspector. We will document that, not build on it.

### 4. go-ios, and whether Forge Device can carry the automation

go-ios has `ios runwda`, `ios tunnel start --userspace` (no root on iOS 17.4 and later) and `ios image auto`. It is a good reference and manual fallback. But adopting it means a second device stack, in Go, next to the `idevice` one Forge Device uses. Its `image auto` also downloads DDIs from a third-party mirror, which we don't want ([device project review](https://claude.ai/artifact/HNeGYGvyhzqjGPgLdxGdgi)).

The question was whether the Forge Device connection could carry the automation. It splits into two parts:

- **The device transport: yes.** Forge Device reaches the phone through usbmuxd with the `idevice` crate. The same crate, at the same pinned version, provides `xctest` (runner launch through `testmanagerd`), `wda` (a WDA client that connects to the phone's port 8100 through usbmuxd), `mobile_image_mounter` (personalised DDI mounting, with Apple's TSS signing), and `core_device` services (`screencaptureservices` screenshots, `app_service` launch). Forge UI Testing adds features to that dependency; it doesn't add a new one.
- **The app session from PR #57 (`forge-devtools`): yes, but for state, not touches.** An app can't synthesise touches into itself or into system alerts through public API. That is why XCTest runs in a separate runner. But the devtools session is the right channel for the second half of the problem, *observing whether the app behaved correctly*. Tests can assert on Rust state through probes (`forge device snapshot`), stream app logs, and run dev-only commands such as resetting data or seeding fixtures. Forge UI Testing uses both: WDA for the UI, devtools for state and setup.

**Verdict: build on `idevice` and Forge Device. Keep go-ios as a manual cross-check when something fails.**

### 5. An in-app automation agent (rejected as the main route)

An agent compiled into dev builds (like KIF or EarlGrey) could walk the app's own accessibility tree and call `accessibilityActivate()` without any extra app. That avoids the app slot and the DDI. It can't see or tap system alerts (permissions, share sheets, the keyboard's own UI), can't launch, background or relaunch the app, and touches are not real touches. We keep it in mind as a fallback if a free-account phone has no slot to spare, but it is not M0.

### 6. Apple's accessibility audit service (Accessibility Inspector)

Added after #79 ([research note](research/ax-audit.md)). The service behind Xcode's Accessibility Inspector starts over plain lockdown through usbmuxd, with no tunnel and no extra app. It can walk the foreground app's elements in VoiceOver order (label, identifier, value), press an element, run Apple's own accessibility audit, and change accessibility settings such as text size. It cannot type, it is slower than WDA's one-call `/source` (about four round trips per element), and it is unknown whether it sees system alerts.

How it fits:
- **WDA stays the M0 route and the only one that can type**, launch, background and relaunch apps, and handle system alerts.
- **If tonight's probe shows SwiftUI identifiers come back**, a second `Driver` backend (`AxDriver`) can serve `forge ui tree`, element checks and presses with no app slot and no 7-day expiry. A test that only reads and taps could then run without the runner. `forge ui` would choose the backend per command and fall back to WDA for the rest.
- **If Apple's audit runs from Linux**, it replaces the basic tree audit in M3, and its settings calls give the Dynamic Type runs without launch arguments.

Nothing in M0 changes: its code and steps are the same either way.

## Free Apple account limits

| Limit | Effect on UI testing | What Forge does |
|---|---|---|
| 3 sideloaded apps per device | The WDA runner is its own app, so it takes a slot. A phone can hold the runner plus two ixOS apps | `forge ui setup` says so before installing and lists what is installed. One runner serves every app, and it is never per-app |
| 10 new App IDs per 7 days | Installing the runner registers one App ID (`XTL-….WebDriverAgentRunner.xctrunner`), and reinstalls reuse it | Keep the runner's bundle id stable |
| 7-day signing | The runner stops launching after a week, like every free-account app | `forge ui setup` reads the installed profile's expiry and re-installs when it's under a day |
| Entitlements | The runner needs no capability entitlements. Development signing gives `get-task-allow`, which is all the runner launch needs | Nothing beyond what xtool already signs |
| Developer Mode | Required, already on for Cati | `forge doctor` already reports it |
| DDI | Must be mounted for XCTest. Personalised for iOS 17+, signed by Apple per device, and gone after a reboot | `forge ui setup --ddi <folder>` mounts it; the folder comes from the DDI mirror (see above) |

The DDI needs one extra input. Older Xcodes ship it in `Xcode.app/Contents/Resources/CoreDeviceDDIs/`, but xtool's SDK builder keeps only `Contents/Developer` (`Sources/XToolSupport/SDKBuilder.swift`, `SDKEntry.wanted`), and the Xcode 26.0 xip has no such folder. The developer downloads a personalised DDI folder once (BuildManifest.plist, Image.dmg, Image.dmg.trustcache) and passes it with `--ddi`. Forge does not download it by itself.

## Design

```
Linux
  forge test ios / forge ui …          (forge-cli)
  forge-uitest                          test runner, locators, assertions, reports, screenshot diff
    ├─ Driver trait ── WdaDriver        WebDriver calls over usbmuxd (idevice `wda`)
    ├─ Runner                           DDI mount + XCTest runner launch (idevice `xctest`, `mobile_image_mounter`)
    └─ AppState (optional)              forge-devtools probes and commands (PR #57)
          │  usbmuxd (USB), the phone's pairing record
iPhone
  WebDriverAgentRunner (XCTest)  ── accessibility tree, touches, keys, alerts, screenshots
  the app under test             ── SwiftUI with accessibility identifiers; Rust core; devtools in dev builds
```

### Crate: `forge-uitest`

A host-only crate next to `forge-device`. It depends on `idevice` at the same exact pin and adds the features `xctest`, `wda`, `mobile_image_mounter`, `tss` and `core_device_proxy`. It reuses `forge-device` for picking the phone, and it doesn't change `forge-device` itself.

- `Locator` is the only way a test names an element: `Id("library.add")` first, then `Label("Add Book")` or `LabelContains(...)`. Coordinates are not a locator, and there is no coordinate tap. Matching by element type comes later if tests need it.
- `Driver` trait (built): `launch`, `activate`, `terminate`, `background(secs)`, `find_all`, `tap`, `type_text`, `clear`, `attribute`, `tree`, `screenshot`. Planned for M1–M2: `swipe`, `scroll_to`, `accept_alert`, `dismiss_alert`. `WdaDriver` implements it. A `FakeDriver` lets the runner, assertions and reports be tested in CI without a phone, the same way `forge-device` tests its CLI against a fake backend.
- Waiting is built in: finding polls until a timeout (default 10 s), so tests never sleep. An element that goes stale between the find and the tap (SwiftUI re-rendered) is found again once; a second stale is a failure. An element going away is often the expected result, not an error: if WDA answers a tap with "stale element reference" and the element is then gone (Done closed the keyboard, Save dismissed the sheet), the tap step passes and the next step's expectation checks the outcome. `expect = { ..., gone = true }` counts a search that WDA answers "stale" twice in a row as gone, and a duplicate or an expectation whose element goes stale mid-read is read again rather than failed.
- `tree` returns a typed `UiNode { kind, identifier, label, value, enabled, visible, frame, children }` parsed from WDA's `/source`. Tests use it for assertions, and the accessibility audit and exploratory testing use it as their model of the screen.

### Tests as files

Deterministic tests are plain data, so they can be reviewed, diffed and later generated:

```toml
# examples/pocket-library/uitests/add-book.toml
name = "Add a book"
app = "org.forgemobile.pocketlibrary"
timeout = 10                     # seconds to wait for an element (default 10)

[[step]]
launch = {}                      # or launch = { args = ["-UIPreferredContentSizeCategoryName", "UICTContentSizeCategoryAccessibilityXXXL"] }

[[step]]
tap = { id = "library.add" }

[[step]]
type = { id = "bookForm.title", text = "Hello from Forge", clear = true }

[[step]]
tap = { id = "bookForm.save" }

[[step]]
expect = { label_contains = "Hello from Forge" }

[[step]]
screenshot = "after-add"
```

Steps that work today, one action per step (unknown keys are rejected):

| Step | Form |
|---|---|
| `launch` | `{}` or `{ args = [...] }` (launch arguments; with arguments a running app is stopped first, so they apply) |
| `activate`, `terminate` | `{}`: bring the app to the front, or quit it. `terminate` then `launch` is a relaunch |
| `background` | seconds, then the app comes back |
| `tap`, `clear` | a target |
| `type` | a target plus `text`, and `clear = true` to empty the field first |
| `scroll` | `{ id = "…" }` scrolls until that element is on screen; `{ id = "list", direction = "down" }` scrolls the scroll view `list` (`up`, `down`, `left`, `right`) |
| `expect` | a target plus at most these checks: `value` (equals), `has_label` (equals), `enabled` (true or false), `gone = true`. A target alone checks that it exists |
| `screenshot` | a name |
| `tree` | `true`: save the accessibility tree |

A target is exactly one of `id` (the accessibility identifier), `label` (exact) or `label_contains`. In your own apps, use these: every control has an identifier ([AGENTS.md](../AGENTS.md) rule 4). For apps Forge didn't build, whose controls often have neither, two more:

| Target | Finds |
|---|---|
| `{ kind = "TextView" }` | Elements of that type (`Button`, `TextField`, `TextView`, `SearchField`, `Cell`…); with several, the best control, as for a duplicate id |
| `{ kind = "Button", label = "Send" }` | That type with that exact label |
| `{ kind = "Button", index = 1 }` | The nth of that type on screen, from 0, in tree order (visible or not). The keyboard and new rows shift it, so prefer a label |
| `{ near = "Message or hold to speak" }` | The text input nearest that text: a field with the text as a placeholder laid over it, or under or after it as a caption, within 120 pt. Add `kind` for another type (`{ near = "Think", kind = "Button" }`). It reads the tree before each search, so it costs one more round trip |

Every search runs again until the timeout, so a control that only appears after typing (a chat app's Send) is found by the next step. In another app, find Send by `label` (or `kind` plus `label`), not `id`: on iOS 26 the keyboard's return key is a Button with identifier `Send` and label "send" (seen in DeepSeek on Cati), and WebDriverAgent's identifier search can match it too. `label` is exact and case-sensitive, so `label = "Send"` reaches only the app's button. When an identifier matches several elements, as when SwiftUI also puts it on a wrapper, steps act on the first control (a Button, a field…) rather than the container. `forge ui tree` reports a `duplicate_identifier` warning for two separate elements with one identifier, but not for a wrapper around an element with the same identifier, which the app can't avoid. Likewise the unlabelled UIKit switch inside a labelled SwiftUI `Toggle` isn't reported as `no_label`. Rows of a list that share one identifier can't be told apart: give each row its own (`timeline.row.<post id>`) or check them by `label_contains`.

Each step costs one or more round trips to WebDriverAgent of a few hundred milliseconds: the front-app check, finding the element, and for a tap the hittable check and the tap itself (about 2 s per tap on Cati). To keep a run fast, don't tap a field before `type`: typing focuses it. Identifiers are found with WDA's `accessibility id` strategy (XCTest's own identifier query) rather than a predicate over the whole tree.

Most of a step's time in a big app (the Claude app's chat, a long list) is WebDriverAgent taking a fresh snapshot of the whole app: every element search does, and so does every attribute read, because WDA resolves the element again each time. A run in another app (`--any-app`) reads the label, identifier and type of each target for the [commit gate](#apps-forge-didnt-build), which used to be three more snapshots per tap or `type`. Such runs now ask the search to return those three with its matches (WDA's `shouldUseCompactResponses` and `elementResponseAttributes` settings), so the gate reads them without asking the phone again; what it decides is unchanged. Anything that can change the screen (a tap, typing, a scroll, a launch) drops what a search said, so later reads ask the phone. Typing itself is WDA's: if the field doesn't have keyboard focus, WDA taps it, takes another snapshot and types. On Cati (2026-11-08, the Claude app, one UI session) a type step went from 2.4 s to 1.3 s and a clear step from 1.2 s to 0.5 s. Waiting less for the app to settle (`waitForIdleTimeout` 1 s instead of 10 s) made no difference.

To see where a run's time goes, set `FORGE_UI_TIMINGS=1`: each WebDriverAgent request is printed to stderr with its time, without element ids or typed text:

```text
          wda   412 ms  POST /elements
          wda  2310 ms  POST /element/:e/value
```

For measuring only, `FORGE_UI_WDA_SETTINGS` adds WDA settings to the ones Forge sets at the start of a run, as a JSON object, for example `FORGE_UI_WDA_SETTINGS='{"waitForIdleTimeout": 1}'` to wait less for the app to settle before each event. It is not a supported option. A setting stays on the runner until it is set again or the runner stops, so set it back (`{"waitForIdleTimeout": 10}`, WDA's default) or run `forge ui stop` after measuring.

Planned, not built (a test using them fails to parse): `swipe`, `alert` (accept or dismiss), `probe` (a devtools state assertion), `command` (a devtools setup command) and `wait_log` (a log line from `forge device logs`). Rust tests can call the same API directly for anything the file format can't express.

### CLI

M0 has these (built):

```
forge ui setup [--ddi DIR] [--runner APP]   mount the DDI if needed, install the runner with xtool, check it starts
forge ui tree  [--app BUNDLE] [--any-app]   the accessibility tree of the screen, plus unlabelled controls
forge ui screenshot [out.png] [--any-app]   save a screenshot on Linux
forge ui run FILE [--out DIR] [--any-app]   run a test file; screenshots and failure details go to DIR
forge ui start [--idle-minutes N]           keep one runner running for the commands above (see "One runner for many commands")
forge ui status | stop                      show or end that session
```

`--any-app` lets these reach apps Forge didn't build, under the person's policy ([below](#apps-forge-didnt-build)).

Later milestones add:

```
forge ui setup --refresh                    re-sign an expiring runner
forge ui audit --app BUNDLE                 the full accessibility audit (M3)
forge test ios [--suite DIR|FILE] [--report DIR]
```

`forge test` already runs an app's Rust and host tests, so M1 decides whether UI suites join it (`forge test ios --device`) or stay under `forge ui`.

`forge test ios` writes a report directory: `report.json`, per-step screenshots, the UI tree at each failure, the app's log lines for the run (from Forge Device), and any new crash reports. A failing step names the locator and shows the nearest matches from the tree ("no element with id `bookForm.save`; nearby buttons: `Save` (no id), `Cancel` (no id)"). The output is readable without the screenshots.

### Accessibility conventions

These go into [ios-ui.md](ios-ui.md#accessibility) and the `forge init` starter:

- **Identifiers on every interactive element and every row**, as `screen.element` (`library.add`, `bookForm.title`, `chat.composer.send`). Rows carry a stable id plus the model's id where it matters (`library.row.<bookId>`). Identifiers are for tests and are never shown or spoken.
- **Labels are for people.** Icon-only buttons use `Button("Add Book", systemImage: "plus")`, so VoiceOver says "Add Book" and the test can fall back to the label. Never set an identifier instead of a label.
- **Combine rows** into one element (`.accessibilityElement(children: .combine)`) with a sensible label and value, so both VoiceOver and the tests see "Dune, Frank Herbert, reading, page 120" as one thing.
- **Dynamic Type**: tests can run at the largest accessibility size (a launch argument, `-UIPreferredContentSizeCategoryName UICTContentSizeCategoryAccessibilityXXXL`) and the audit flags truncated text.

`forge ui audit` checks the current screen's tree for: buttons and controls with no label, images with no label that aren't hidden, interactive elements with no identifier (a test-robustness warning, not an accessibility error), duplicate identifiers, and tap targets smaller than 44 × 44 points. It runs inside test suites too, so each screen a test visits is audited for free.

### Exploratory testing (later, separate)

The exploratory tester is a separate command (`forge explore ios`). It never takes part in pass/fail acceptance. It reads the UI tree, picks valid actions (taps on enabled elements, text into fields, back navigation, background and resume), and watches for crashes (Forge Device crash reports), error alerts, error log lines and devtools state invariants. When it finds a problem it writes the action sequence out as a normal test file, which then becomes a deterministic regression test after a person reviews it. An AI model can choose the actions later, but the first version is seeded random walks so its results are reproducible.

## Milestones

The same milestones serve apps Forge didn't build: [any app on the iPhone](research/general-device-control.md) adds a per-app policy, a few `Driver` calls and locators for apps without identifiers, not a second stack.

| | Goal | Done when |
|---|---|---|
| **M0** | Prove the five basics on Cati | From Linux: launch an installed app, find a SwiftUI button by accessibility identifier, tap it and enter text, read the resulting UI state, and save a screenshot on Linux. Recorded in the ledger by Reuben's run |
| M1 | `forge test ios` with test files and reports | Pocket Library's add/edit/delete journey passes on the phone. The report has screenshots, the tree on failure, and app logs |
| M2 | App lifecycle and state | Background and resume, relaunch, alerts (a permission prompt), devtools probes and setup commands. Chat's "send, background, reopen, still there" journey passes |
| M2.5 | Runner-free inspection, if #79's probe passes | `AxDriver` behind `Driver`: `forge ui tree` and read-and-tap tests without the WDA runner |
| M3 | Accessibility pass across the demos | Identifiers and labels in the starter, Pocket Library, Weather and Chat; `forge ui audit` clean on their main screens; a Dynamic Type run; conventions in ios-ui.md |
| M4 | Screenshots and conditions | Screenshot comparison against approved baselines (with masked regions for times and dates): built and tested on Linux, not yet run on a phone ([visual-regression.md](visual-regression.md)); offline and slow-network runs through the instruments condition inducer; performance numbers from the devtools stream |
| M5 | Exploratory testing | `forge ui explore` ([forge-explore.md](forge-explore.md)): seeded walks over a Forge app's screens, crash, alert, dead-end and accessibility checks, and a `forge ui run` replay file per issue. Built and tested on Linux 2026-11-08, not yet run on an iPhone. Still to come: invariants, scrolling below the fold, performance during the walk. Optionally Forge's own Swift XCTest runner if WDA becomes limiting |

### M0 in detail

1. Add `crates/forge-uitest` with `Locator`, `Driver`, `WdaDriver`, the runner launch and DDI mount, plus tests against a fake driver.
2. Add `forge ui setup`, `forge ui tree`, `forge ui screenshot` and `forge ui run`, plus `examples/pocket-library/uitests/m0.toml`, which runs the five M0 steps against Pocket Library and prints each result.
3. Give Pocket Library's library screen and book form accessibility identifiers (and fix any missing labels the tree shows). It is updated in place on the phone, so it needs no extra slot.
4. On Reuben's laptop, over Remote Control: get the DDI, mount it, install the runner with `xtool install`, then run `forge ui run examples/pocket-library/uitests/m0.toml`. Reuben only frees a slot on the phone, unlocks it, and confirms what he sees.

## Running M0

On the Linux machine, with the phone plugged in, unlocked and trusting the computer:

1. Get the runner: `WebDriverAgentRunner-Runner.zip` from [Appium's WDA release v16.15.3](https://github.com/appium/WebDriverAgent/releases/tag/v16.15.3). Check its sha256 (above) and unzip it.
2. Get a personalised Developer Disk Image: clone [doronz88/DeveloperDiskImage](https://github.com/doronz88/DeveloperDiskImage) and use its `PersonalizedImages/Xcode_iOS_DDI_Personalized` folder. Xcode 26.0's `Xcode.xip` has no `CoreDeviceDDIs` folder.
3. Make room: a free account allows three sideloaded apps, and the runner is one. On the phone, turn on **Settings > Developer > Enable UI Automation** (it appears once Developer Mode is on). Without it, launching and every touch fail with WDA error code 41, "Not authorized for performing UI testing actions".
4. `forge ui setup --ddi <DDI folder> --runner WebDriverAgentRunner-Runner.app` mounts the image, has xtool sign and install the runner, and checks it starts. The first time, the phone may ask you to trust the developer profile again: Settings > General > VPN & Device Management. If the runner never starts, `forge ui setup` stops after 120 seconds and says so; it does not hang.
5. Install Pocket Library with its identifiers (`forge run ios --project examples/pocket-library`).
6. `forge ui run examples/pocket-library/uitests/m0.toml` opens the New Book form, types a title, checks the field and the Add button, prints the screen's tree, saves screenshots in `forge-ui-out/`, and cancels.

After the phone restarts, run `forge ui setup --ddi …` again (the image is unmounted at every reboot). After seven days, reinstall the runner with `--runner`.

`forge ui setup` words its app-limit and expiry notes for the team xtool signs with: `--team free|paid`, or `[ios.signing] team_type` in the forge.toml of the directory it runs in. A paid Apple Developer Program team has no 3-app limit and its signatures last as long as the provisioning profile (up to a year), so the runner can stay installed. Without either setting it assumes nothing and says what a free account would hit.

### One runner for many commands

Without a session, every `forge ui` command starts the WebDriverAgent runner and the phone tears it down when `forge` exits. On Cati that is about 2 s per command (G0, 2026-11-08), most of a short test, and an agent exploring an app makes hundreds of calls. A **UI session** keeps one runner running instead:

```
forge ui start                 # starts `forge ui serve` in the background; returns when WDA is ready
forge ui tree / run / …        # use the session's runner: no runner start, no disk-image check, no app list
forge ui status                # pid, state, WDA version, uptime, last use, restarts, log path (doesn't touch the phone)
forge ui stop                  # stops it; the runner shuts down at once
```

- Commands find the session through a small state file per phone (`$XDG_RUNTIME_DIR/forge/ui-session/<udid>.json`, else `~/.cache/forge/ui-session/`, or `$FORGE_UI_SESSION_DIR`). Before using it they check that the session's process is alive and that WDA answers `/status` within 5 s. A file left by a dead process is removed and the command starts its own runner, as before. The file holds no screen content: process id, phone, runner, state and installed bundle ids.
- Each command still opens its own WebDriver session on the shared runner (one small request), so commands never inherit another command's element ids.
- The session lists installed apps when it starts. An app installed since is looked up on the phone again when a command names it.
- **It stops** on `forge ui stop`, Ctrl-C or SIGTERM, after 30 minutes with no command (`--idle-minutes N`, 0 for never), or when the process given in `--exit-with <pid>` exits.
- **It repairs itself.** Every 10 s it asks WDA for `/status` (one local USB request). After two misses in a row it restarts the runner; after three failed restarts in a row it gives up and exits. A command that arrives while the runner is starting or restarting waits up to 150 s for it, reading only the state file, then says what happened and points at `forge ui status`.
- `forge ui setup` uses a running session's runner for its check. Reinstalling the runner (`--runner … --reinstall`) refuses while a session is running: `forge ui stop` first.
- **`forge mcp` starts one for its UI tools** with `--exit-with <its own pid>`, so the runner lives exactly as long as the MCP server (or 30 idle minutes). If the session can't start, the tools start their own runner as before and the server tries again after 5 minutes.
- `forge ui serve` is the same thing in the foreground, with its log on the terminal.

On Cati (2026-11-08, WDA 16.15.2): `forge ui start` took 1.5 s; `forge ui tree` took 0.5 s with the session and 1.4 to 1.9 s without. A test file saves one runner start (m0: 9.2 s against 10.1 s), since its steps already shared one runner. Stop, status, and recovery after the session process was killed (`kill -9`; the next command started its own runner) worked as described.

Nothing about which app a command may act on changes: the front-app check below runs on every command, session or not.

### Only the app under test

A test acts only on its own app. Before every tap, typing, expectation and tree read, `forge ui run` asks WebDriverAgent which app is in front (`GET /wda/activeAppInfo`). It waits up to the step timeout for the test's app, then fails with "`<app>` is not in front (`<other>` is); refusing to act on another app". There are two exceptions. `expect = { ..., gone = true }` also passes after the app was terminated, and a screenshot is taken whatever is in front. `forge ui tree --app <bundle id>` activates the app and makes the same check. Without `--app`, `ui tree` and `ui screenshot` say which app was in front, and so does their `--json` (`data.app`). A future runner-free driver (M2.5) must make the same check before it presses or audits anything.

### Apps Forge didn't build

WebDriverAgent drives any installed app, so a test file can name an App Store app (G0 typed and sent a message in the Claude app on Cati this way, 2026-11-08). Forge doesn't let that happen by accident. The design is in [general-device-control.md](research/general-device-control.md#safety); the code is `forge-uitest`'s `policy` module, which `forge ui` uses and the MCP server reaches through it.

**Your own apps are unchanged.** An app is yours when its installed id has xtool's `XTL-` prefix (free team), or the `forge.toml` in the current folder or in the app folder holding the test file names it, or your policy file marks it `full`. Everything works as before for these.

**Every other app needs `--any-app`.** Without it, `forge ui run` refuses a test file that names one, and `ui tree` and `ui screenshot` refuse to read the screen while one is in front (the home screen is fine). The MCP server never passes `--any-app`, so an agent can't reach another app through it.

**Then a per-app level, from a file you write:** `~/.config/forge/device-control.toml` (or under `$XDG_CONFIG_HOME`). It is outside every project, so an agent editing a project can't loosen it.

```toml
default = "ask"                     # apps not listed (also the default with no file)

[apps]
"com.apple.weather" = "operate"
"com.example.bank" = "observe"
"org.forgemobile.agentlab" = "full"
```

| Level | Allows |
|---|---|
| `deny` | Nothing |
| `observe` | Launch, bring to the front, background, tree, screenshot, scroll, expectations |
| `operate` | Also tap, type, clear, stop. **Commits ask every time** |
| `full` | Everything except the hard refusals. Counts as your own app |
| `ask` | Forge asks at the terminal which level to use for this run (deny, observe or operate). With no terminal (an agent, a script), refused |

**The commit gate.** At `operate`, before a tap Forge reads the target's label, identifier and kind. If a word in them is a commit (send, post, reply, share, publish, submit, pay, buy, purchase, order, checkout, subscribe, transfer, donate, delete, remove, erase, block, report, call, invite, follow, like, allow…, with plain endings such as *sends*, *deleting*), Forge asks "Allow it? [y/N]" on the terminal. Typing text with a newline counts too, because Return sends in many apps. Tapping a text field only focuses it and is never a commit. A tap on any other control with neither a label nor an identifier (one found by `kind`, `index` or `near`) counts as a commit too, because nothing says what it does: it could be Send. With nobody at a terminal, a commit is refused, and the step fails with the reason. The list will miss some words, which is why money and messaging apps should stay at `observe`.

**Never, at any level:** Settings, Wallet and the Apple Pay sheet, Passwords, the App Store and its purchase sheet, Find My, Phone, FaceTime, and the Face ID and passcode prompt; typing into secure fields (passwords, passcodes, codes) of another app.

**Privacy.** A run in another app writes its screenshots, failure trees and journal to `~/.cache/forge/any-app/<UTC>-ui-run/` (or `--out`), never into an app's `.forge/runs/`, and writes no evidence bundle. The journal, `journal.jsonl`, has one line per decision: time, app, action, the target's label, identifier and kind, and allowed, approved or refused with the reason. Typed text is recorded only as its length. On a failure, the tree and screenshot are saved only if the app under test is still in front, never whatever else came up. Apple's accessibility service (`ax_probe`) switches app monitoring on only for its walk and drops its reports about other apps.

Checked on Cati (iPhone 15 Pro Max, iOS 26.3.1) on 2026-11-08 with the Claude app at `operate`. Typing worked. Send stopped at the prompt and was refused when the answer was n. With no terminal it was refused outright, and without `--any-app` nothing ran. The journal recorded the Send as refused, with the typed text's length only. Pocket Library's M0 still passed 11/11 without the flag. Not yet shown on the phone: `ax_probe` cleaning up after a Ctrl-C in the middle of a walk (the interrupt landed before the walk began).

### What a failing run gives an agent

`forge ui run` exits non-zero on the first failing step and never reports success for a step it could not check. For each failure it prints the step, the error (for a missing element, the locator; for a wrong value, what was expected and what was found), and the elements on screen that a test could target instead. It also saves the screen as `failure-stepN-tree.txt` and `failure-stepN-tree.json`, and a screenshot as `failure-stepN.png`. With `forge --json`, the result's `data` has the same: `failed_step.{step, action, error, on_screen, tree}` and the screenshot paths. `forge --json ui tree` returns the whole tree as JSON nodes (`kind`, `identifier`, `label`, `value`, `enabled`, `visible`, `frame`, `children`) plus the audit findings.

### When the phone refuses

- **`Not authorized for performing UI testing actions` (XCTDaemonErrorDomain 41)** on every action while `forge ui setup` reports WDA ready: turn on Settings > Developer > Enable UI Automation on the phone (iOS 17 and later).
- To see what the phone's test daemon answered, run any `forge ui` command with `FORGE_UI_DEBUG=1`. It prints `idevice`'s debug log to stderr, including `authorize_test reply: …` for the runner's UI testing authorisation. `idevice` treats any answer other than `false` there as success.

### Checked on the phone

Everything above is also tested on Linux against a fake driver and canned WebDriverAgent replies. On 2026-11-07 these parts were checked on Cati (iPhone 15 Pro Max, iOS 26.3.1):

| Part | Result |
|---|---|
| `forge ui setup --ddi` | Mounted a personalised DDI from the doronz88 mirror through `idevice` and Apple's TSS |
| `forge ui setup --runner` | `xtool install` signed the unsigned WDA 16.15.2 runner under the free team as `XTL-<team>.com.facebook.WebDriverAgentRunner.xctrunner` |
| Runner start | `idevice`'s XCTest launch through `CoreDeviceProxy`; WDA ready in about 3 s. A runner left by the previous command is shut down first |
| Front-app check | `/wda/activeAppInfo` names the `XTL-` bundle id |
| `uitests/m0.toml` | 11 of 11 steps in 10 s: identifiers through `name == …` predicates (WDA 16 rejects `identifier`), the toolbar `Menu` and its item, typing into a vertical `TextField`, `enabled` |
| Screenshots | 1290x2796 PNGs, intact on Linux |

Not yet checked on a phone: a paid team, the 7-day expiry and `--reinstall`, other apps' test files, and the accessibility audit beyond missing labels and identifiers.

## Risks

- **`idevice`'s XCTest support is young** (it shipped in 0.1.6x). It may not yet handle iOS 26.3. Fallback: start WDA once with `go-ios runwda` to separate a runner problem from a Forge problem, and keep Forge's WDA client, which only needs the port.
- **WDA bundle-id rewriting.** If the runner rejects xtool's `XTL-` bundle id, we rewrite the Info.plist before signing (a supported WDA option, `updatedWDABundleId`, does the same thing in Appium).
- **The DDI and TSS.** Personalised mounting calls Apple's TSS server per device and per boot. It needs network access, and Apple could change it. Mac users have the same dependency through Xcode.
- **Upstream xtool.** Nothing here needs changes in xtool. If extracting the DDI belongs in xtool, it goes on Reuben's fork first; no upstream PRs or issues.
- **Liquid Glass and SDK stamping.** Linux xtool builds may run in compatibility mode on iOS 26 ([ios-ui.md](ios-ui.md)). Screenshots taken by M4 baselines must come from the same kind of build.
