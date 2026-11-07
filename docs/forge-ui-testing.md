# Forge UI Testing: driving a real iPhone from Linux

Status: 2026-11-07. M0 is built (`crates/forge-uitest`, `forge ui`) and **partly working on a phone**. On Cati (iPhone 15 Pro Max, iOS 26.3.1), with #90's fix for a tokio panic in `ui setup`: the disk image mounts, the runner starts, and launch (Pocket Library came to the front by itself, confirmed by Reuben), `ui tree` and `ui screenshot` work. That needed **Settings > Developer > Enable UI Automation** on the phone; without it WDA answers code 41, "Not authorized for performing UI testing actions". Still open: finding by identifier uses an attribute WDA 16 rejects, and the runner dies when the `forge` process exits, so `m0.toml` has not passed. The accessibility service (route 6) did read Pocket Library's identifiers on Cati. Everything else on this page is tested on Linux only, unless it says so.

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
- Waiting is built in: finding polls until a timeout (default 10 s), so tests never sleep. An element that goes stale between the find and the tap (SwiftUI re-rendered) is found again once; a second stale is a failure.
- `tree` returns a typed `UiNode { kind, identifier, label, value, enabled, visible, frame, children }` parsed from WDA's `/source`. Tests use it for assertions, and the accessibility audit and exploratory testing use it as their model of the screen.

### Tests as files

Deterministic tests are plain data, so they can be reviewed, diffed and later generated:

```toml
# examples/pocket-library/uitests/add-book.toml
name = "Add a book"
app = "org.forgemobile.pocketlibrary"

[[step]]
launch = { fresh = true }

[[step]]
tap = { id = "library.add" }

[[step]]
type = { id = "bookForm.title", text = "Hello from Forge" }

[[step]]
tap = { id = "bookForm.save" }

[[step]]
expect = { id = "library.row", label_contains = "Hello from Forge" }

[[step]]
screenshot = "after-add"
```

Steps cover: `launch`, `tap`, `type`, `clear`, `swipe`, `scroll_to`, `expect` (exists, gone, label, value, enabled), `alert` (accept or dismiss), `background` (seconds), `relaunch`, `screenshot`, `probe` (a devtools state assertion), `command` (a devtools setup command) and `wait_log` (a log line from `forge device logs`). Rust tests can call the same API directly for anything the file format can't express.

### CLI

M0 has these (built):

```
forge ui setup [--ddi DIR] [--runner APP]   mount the DDI if needed, install the runner with xtool, check it starts
forge ui tree  [--app BUNDLE]               the accessibility tree of the screen, plus unlabelled controls
forge ui screenshot [out.png]               save a screenshot on Linux
forge ui run FILE [--out DIR]               run a test file; screenshots and failure details go to DIR
```

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

| | Goal | Done when |
|---|---|---|
| **M0** | Prove the five basics on Cati | From Linux: launch an installed app, find a SwiftUI button by accessibility identifier, tap it and enter text, read the resulting UI state, and save a screenshot on Linux. Recorded in the ledger by Reuben's run |
| M1 | `forge test ios` with test files and reports | Pocket Library's add/edit/delete journey passes on the phone. The report has screenshots, the tree on failure, and app logs |
| M2 | App lifecycle and state | Background and resume, relaunch, alerts (a permission prompt), devtools probes and setup commands. Chat's "send, background, reopen, still there" journey passes |
| M2.5 | Runner-free inspection, if #79's probe passes | `AxDriver` behind `Driver`: `forge ui tree` and read-and-tap tests without the WDA runner |
| M3 | Accessibility pass across the demos | Identifiers and labels in the starter, Pocket Library, Weather and Chat; `forge ui audit` clean on their main screens; a Dynamic Type run; conventions in ios-ui.md |
| M4 | Screenshots and conditions | Screenshot comparison against approved baselines (with masked regions for times and dates); offline and slow-network runs through the instruments condition inducer; performance numbers from the devtools stream |
| M5 | Exploratory testing | `forge explore ios` with seeded walks, crash and invariant detection, and saved reproductions. Optionally Forge's own Swift XCTest runner if WDA becomes limiting |

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

### Only the app under test

A test acts only on its own app. Before every tap, typing, expectation and tree read, `forge ui run` asks WebDriverAgent which app is in front (`GET /wda/activeAppInfo`). It waits up to the step timeout for the test's app, then fails with "`<app>` is not in front (`<other>` is); refusing to act on another app". There are two exceptions. `expect = { ..., gone = true }` also passes after the app was terminated, and a screenshot is taken whatever is in front. `forge ui tree --app <bundle id>` activates the app and makes the same check. Without `--app`, `ui tree` and `ui screenshot` say which app was in front, and so does their `--json` (`data.app`). A future runner-free driver (M2.5) must make the same check before it presses or audits anything.

### What a failing run gives an agent

`forge ui run` exits non-zero on the first failing step and never reports success for a step it could not check. For each failure it prints the step, the error (for a missing element, the locator; for a wrong value, what was expected and what was found), and the elements on screen that a test could target instead. It also saves the screen as `failure-stepN-tree.txt` and `failure-stepN-tree.json`, and a screenshot as `failure-stepN.png`. With `forge --json`, the result's `data` has the same: `failed_step.{step, action, error, on_screen, tree}` and the screenshot paths. `forge --json ui tree` returns the whole tree as JSON nodes (`kind`, `identifier`, `label`, `value`, `enabled`, `visible`, `frame`, `children`) plus the audit findings.

### What still needs the phone

Everything above is tested on Linux against a fake driver and canned WebDriverAgent replies. These parts have only been checked against the source of `idevice` and WDA, and need a run on Cati:

| Part | What the run proves |
|---|---|
| `forge ui setup --ddi` | Personalised DDI mounting through `idevice` and Apple's TSS on iOS 26.3 |
| Front-app check | `/wda/activeAppInfo` names the `XTL-` bundle id of the app in front |
| `forge ui setup --runner` | `xtool install` signs the unsigned WDA runner, its `XTL-` bundle id is accepted, and it fits in the app slots |
| Runner start | `idevice`'s XCTest launch through `CoreDeviceProxy` on iOS 26.3, and WDA answering on port 8100 through usbmuxd |
| `uitests/m0.toml` | SwiftUI identifiers reach WDA (`identifier` predicates), a toolbar `Menu` and its items can be tapped, typing into a vertical `TextField` works, `enabled` reads correctly |
| `forge ui screenshot` | The PNG arrives intact on Linux |

## Risks

- **`idevice`'s XCTest support is young** (it shipped in 0.1.6x). It may not yet handle iOS 26.3. Fallback: start WDA once with `go-ios runwda` to separate a runner problem from a Forge problem, and keep Forge's WDA client, which only needs the port.
- **WDA bundle-id rewriting.** If the runner rejects xtool's `XTL-` bundle id, we rewrite the Info.plist before signing (a supported WDA option, `updatedWDABundleId`, does the same thing in Appium).
- **The DDI and TSS.** Personalised mounting calls Apple's TSS server per device and per boot. It needs network access, and Apple could change it. Mac users have the same dependency through Xcode.
- **Upstream xtool.** Nothing here needs changes in xtool. If extracting the DDI belongs in xtool, it goes on Reuben's fork first; no upstream PRs or issues.
- **Liquid Glass and SDK stamping.** Linux xtool builds may run in compatibility mode on iOS 26 ([ios-ui.md](ios-ui.md)). Screenshots taken by M4 baselines must come from the same kind of build.
