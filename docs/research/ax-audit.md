# Spike: Apple's accessibility audit service from Linux

Status: **probe built, not yet run on a phone** (2026-11-07). This is discovery D1 / action A2 of the [agent capability audit](forge-agent-capability-audit.md), taken one step further: there is now a Rust probe that talks to the service, for the next phone session.

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
| `[4] audit: N issues` | Apple's audit ran |

The decoding and archiving are unit-tested on Linux (`cargo test -p forge-device --features ax-probe --example ax_probe`). Nothing here has met a real phone yet.

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
