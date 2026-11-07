# Spike: Apple's accessibility audit service from Linux

Status: **reading the screen and pressing by identifier work on a physical iPhone without the Developer Disk Image** (Cati, iOS 26.3.1, 2026-11-07). Experimental: it is a cargo example behind a feature flag, not a `forge` command. The audit is not yet working, with or without the DDI mounted; the probe fixes for it (#88, merged) still need a run on the phone. This is discovery D1 / action A2 of the [agent capability audit](forge-agent-capability-audit.md).

## Results so far

Run 1 was on Cati (iPhone 15 Pro Max, iOS 26.3.1) on 2026-11-07, with **no DDI mounted**. Pocket Library was built from #61's branch and in front.

| Stage | Result |
|---|---|
| [1] Start the service | Works. Lockdown answered with `ssl true`, and the TLS-then-plaintext handshake worked. One later run failed the handshake (`InvalidContentType`) straight after a previous run, so the probe now retries once |
| [2] Capabilities, audit types | Works (`hostApiVersion`, list of audit types) |
| [3] Element walk | **Works.** 10 elements in 514 ms, 12–24 ms per element including three attribute reads. **SwiftUI identifiers come through:** `library.add`, `library.empty.add`, `library.filter`. Labels and values come through too, for example the search field's placeholder. On an empty library the tab bar buttons had no identifier. With a book in the library they reported their SF Symbol names (`books.vertical`, `chart.bar`, `gearshape`), so tabs need explicit identifiers in the app. With the Claude app in front it read that app's identifiers too (`BackButton`, `thread-checklist-pill`), so the service reads any foreground app, not only our builds |
| [4] Press | **Works.** `--press library.add` opened Pocket Library's Add menu. The next walk showed `library.add.catalogue`, `library.add.manual`, `library.add.epub` and "Dismiss context menu" |
| [4] Audit | Not working yet, with or without the DDI (run 2 had it mounted). The likely cause is in the probe, not the service. The 120 s wait listed one stray reply: the list of audit types (`testTypeSufficientElementDescription`, `testTypeContrast`, … `testTypeDynamicText`). So replies were shifted by one: the daemon sends two messages on connect, which pymobiledevice3 discards and the probe did not. As a result `deviceBeginAuditTypes:` was sent the capabilities list instead of the audit types. The probe now drains those two messages first, prints them, and refuses to start an audit with anything that isn't a `testType…` list |

What this means: reading the screen and pressing controls need no runner, no app slot and no DDI. Walking a screen is fast enough for `forge ui tree` (well under a second for a typical screen). Text entry still needs WDA.

## Question

Can Forge read the UI of a running app, and press its controls, through the service behind Xcode's Accessibility Inspector (`com.apple.accessibility.axAuditDaemon.remoteserver`), instead of installing WebDriverAgent?

If it can, the gain is mostly about the free Apple team:
- inspection uses no app slot (the WDA runner takes one of three);
- nothing expires after 7 days;
- there is no runner to re-sign;
- we get Apple's own accessibility audit as well.

It runs alongside #61's WebDriverAgent route, not instead of it.

## What the protocol looks like

The protocol comes from pymobiledevice3 11.50.0 (`services/accessibilityaudit.py`). pymobiledevice3 is GPL-3.0, so the probe reimplements the protocol rather than copying code.

- **Transport.**
  - Lockdown starts the service over usbmuxd, the same path `forge device logs` uses. pymobiledevice3 also uses this path on iOS 17+, with no RSD tunnel. Its accessibility commands take a plain lockdown provider, unlike Instruments, which needs the tunnel on 17+.
  - The connection does a TLS handshake and then speaks DTX in plaintext on the same socket (pymobiledevice3's `strip_ssl=True`).
  - The DTX in question is the Instruments message protocol. The `idevice` crate Forge already pins (`=0.1.68`) has a DTX client (`RemoteServerClient`, feature `dvt`), but it keeps TLS on. So the probe does the handshake itself with rustls and passes the raw socket on.
- **Calls** go on the root channel, and the arguments are NSKeyedArchived dictionaries of the form `{ObjectType, Value}`:

  | Call | Purpose |
  |---|---|
  | `deviceCapabilities`, `deviceAllSupportedAuditTypes` | Discovery |
  | `deviceSetAppMonitoringEnabled:`, `deviceInspectorSetMonitoredEventType:` | Start focus events |
  | `deviceInspectorMoveWithOptions:` | Move focus: direction 4 = next, 5 = first |
  | `deviceElement:valueForAttribute:` | Read attributes such as `Label`, `Identifier` and `Value`. Only the focused element resolves |
  | `deviceElement:performAction:withValue:` | Press the element (`AXAction-2010`). pymobiledevice3 says the target process needs `task_for_pid-allow`; for our own development builds that should mean `get-task-allow`, which needs checking |
  | `deviceBeginAuditTypes:` | Run Apple's audit |
  | `deviceAccessibilitySettings`, `deviceUpdateAccessibilitySetting:withValue:` | Read or change settings such as text size |

- **Events** arrive as device-initiated calls:
  - `hostInspectorCurrentElementChanged:` carries the focused element: a caption, plus an opaque element handle in `PlatformElementValue_v1`.
  - `hostDeviceDidCompleteAuditCategoriesWithAuditIssues:` carries the audit results.

The element walk moves the VoiceOver focus one element at a time, so it is ordered, but it is slower than WDA's `/source`, which returns the whole tree in one call. Each element costs about four round trips: one move plus three attribute reads.

## The probe

`crates/forge-device/examples/ax_probe.rs` sits behind a cargo feature so that normal builds don't change. On the laptop, with the phone unlocked and the app under test in front:

```bash
cargo run -p forge-device --features ax-probe --example ax_probe -- --max 40
cargo run -p forge-device --features ax-probe --example ax_probe -- --audit
cargo run -p forge-device --features ax-probe --example ax_probe -- --press library.add
```

It prints one line per stage, so a failure shows how far it got:

| Line | Meaning |
|---|---|
| `[1] service started` | Lockdown started the service. If this fails, note whether the Developer Disk Image was mounted at the time (`forge ui setup` from #61 mounts it) |
| `[2] capabilities / audit types` | The DTX connection and the TLS stripping work |
| `[3] N caption=… Label=… Identifier=… Value=…` | One line per element, with the time per element. **The key check is whether SwiftUI's `.accessibilityIdentifier` shows up as `Identifier`** |
| `[4] pressed …` | Pressing works. Check on the phone that the button actually reacted |
| `[4] audit: N issues` | Apple's audit ran. On a timeout it lists the other events it received |

The decoding, archiving, report and WDA comparison are unit-tested on Linux (`cargo test -p forge-device --features ax-probe --example ax_probe`). The phone results are under "Results so far" above.

### Recording a run

```bash
# WDA's view of the same screen first (#61), then the probe with a report:
forge --json ui tree --app org.forgemobile.pocketlibrary > wda-tree.json
cargo run -p forge-device --features ax-probe --example ax_probe -- --max 60 --audit \
  --json ax-report.json --compare-wda wda-tree.json
```

`ax-report.json` (`forge.ax-probe/1`) holds the stage reached (0–4), the time to start the service, each element in VoiceOver order with its caption, label, identifier, value and time, how the walk ended (`back_to_start`, `max_reached`, `no_focus_events`), whether `--press` worked, the audit issues, and the error if a stage failed. With `--compare-wda`, `compare_wda` lists the identifiers both routes saw, and those only one saw. The file is written even when a stage fails, so a failed run is still evidence. Starting the service has a 20 s limit and every call a 10 s limit, so a silent phone ends the run instead of hanging it.

## Apple's service and WebDriverAgent compared

From the protocol and #61's code. The "Phone" column was the list of open questions; "Results so far" answers the first three.

| | Accessibility audit service (this probe) | WebDriverAgent (#61) | Phone |
|---|---|---|---|
| Setup | Nothing installed on the phone; lockdown over usbmuxd | A signed runner app (one of 3 slots on a free team, re-signed every 7 days) and the Developer Disk Image | Whether the service needs the disk image |
| Hierarchy | A flat list in VoiceOver order, one element per focus move (`includeContainers` asks for containers too). No parent and child structure, no frames | The whole tree in one call: type, identifier, label, value, enabled, visible, frame, children | Whether containers come back |
| Identifiers | `Identifier` attribute of the focused element | `identifier` | **Whether SwiftUI's `.accessibilityIdentifier` comes back** |
| Labels and values | `Label`, `Value`; the caption adds traits ("Add, Button") | `label`, `value` | Yes/no per field |
| Element types | Only as caption text (the trait), not a type field | `kind` (Button, TextField, Cell, …) | |
| Speed | About four round trips per element | One call per screen | Measured ms per element |
| Audit | Apple's own checks (contrast, labels, hit regions, Dynamic Type) | #61's basic audit (no label, no identifier) | Whether `--audit` returns issues |
| Interaction | Press only (`AXAction-2010`), possibly needs `get-task-allow` | Tap, type, clear, swipe, launch, terminate, background | Whether the press reaches the app |
| Text entry, screenshots | No | Yes | |
| Settings | Can read and change accessibility settings such as text size (not tried) | No | |

**They complement each other.** WebDriverAgent stays the route for UI tests, typing, launching and screenshots. If identifiers come back, Apple's service can give `forge ui tree` and an accessibility audit with no runner installed, which matters most on a free team and in every session right after a phone restart. Nothing replaces anything until the phone run says so.

## What the answers decide

| Result on Cati (iOS 26.3.1) | Consequence |
|---|---|
| Stages 1–3 work, identifiers present, no DDI needed | `forge ui tree` can work without the runner. This is the cheapest inspection Forge could have. Map it to #61's `UiNode` behind its `Driver` trait |
| Same, but the DDI must be mounted | Still no app slot. #61 already mounts the DDI |
| `[4] pressed` works | Taps by identifier without WDA. Text entry still needs WDA, because this service has no way to type |
| Audit works | Replaces #61's own basic tree audit (planned for M3) with Apple's |
| Stage 1 fails | Record the error. WDA stays the only route, and the free-team cost of one slot stays |

## Not covered

- **Screenshots** are a separate service: idevice has `screenshotr`, and DVT has `screenshot`.
- **System alerts and other apps:** the service follows the foreground app. Whether it can see permission alerts is unknown.
- **The RSD shim** (`….remoteserver.shim.remote`) isn't tried. pymobiledevice3 uses it only when it is already tunnelling.

## Knowing which app is in front

During run 1 the probe sometimes read the Claude app, which Reuben had open on the same phone. The service reads whichever app is in front, so the tooling has to check, and later ensure, that the app under test is the one in front.

- **Check (in the probe now):** `--app library.` first walks the screen until it finds an identifier with the app's `screen.` prefix. If it finds none, it refuses the audit. A press only goes to an identifier inside that prefix. The probe also prints the daemon's `hostAppStateChanged:` events, to find out whether they name the app; if they do, that gives a direct check. The guard worked on Cati: with Safari in front it refused. The daemon sent no `hostAppStateChanged:` events in any run. **Drawback:** the check walks the accessibility focus, which shows a highlight on the phone.
- **Ensure (needs the DDI, so for #61's driver):** bring the app to the front by bundle id before inspecting it. Candidates in idevice 0.1.68 are CoreDevice `app_service.launch_application` and DVT `process_control.launch_app`; both need the disk image and, on iOS 17+, the userspace tunnel #61 already sets up. WDA can activate apps too.
- **Invisible check (also needs the DDI):** DVT `deviceinfo` running processes mark the foreground app, and WDA has `/wda/activeAppInfo` when the runner is installed. A driver should use one of these rather than the walk, and walk only once the app is in front. Nothing should poll.

## Leaving the phone clean

Run 1 left the inspector on. Each walk enables app monitoring and moves the focus, and the probe never turned monitoring off. Reuben saw the highlight keep moving through whatever app was in front, especially while the session polled the probe every ~11 s. Every run now ends, including after an error, by sending `deviceSetAppMonitoringEnabled:` false, `deviceInspectorShowVisuals:` 0 and `deviceInspectorShowIgnoredElements:` 0. `--off` sends only those, to clear an interrupted run. An interrupted process (Ctrl-C) still can't clean up, so run `--off` afterwards.

A future `AxDriver` should do both: launch or activate by bundle id, confirm the foreground app, then act, and refuse otherwise.
