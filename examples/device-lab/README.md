# Device Lab

A live view of everything an iPhone tells an app about itself: network, Bluetooth, battery and temperature, storage and memory, the motion sensors, a compass, the barometer, location, and what the device is. It also lists what iOS keeps from apps, and why.

It exists to prove Forge's optional native services ([docs/native-services.md](../../docs/native-services.md#optional-services)). The device capabilities are the framework's; this app is their consumer. It is also the one app on the test iPhone that hosts every other example app.

It has three tabs:

- **Apps** (the home screen): the build that is installed, the hosted apps, Interaction Patterns and Open at Launch ([below](#the-other-apps-apps-tab)).
- **This iPhone**: the live sensor dashboard (the table below, down to What iOS Keeps from Apps). Its sensors run only while this tab is on screen.
- **Tools**: what a phone sitting needs, then experiments and developer checks:
  - **Checks**: the next sitting's checks, built into the app (`Sources/AppHost/PhoneChecks.swift`). Tick Pass, Fail or Skip, add a note, and share the results as text. Answers are kept per build number. Whoever prepares a sitting replaces that file's text in the PR the build comes from.
  - **Servers**: whether the chat server (Relay's Chats) and the take server (Soundscape) answer from the phone, at the address each app uses, with the time it took or why not. Tailscale addresses are marked, since Tailscale must be on for the phone too.
  - **Logs**: the last 512 records from Device Lab and every hosted app, kept in memory by Forge's logging pipeline (`rust/src/logs.rs`), with a level filter, search and Share, so a problem can be sent without the cable.
  - **Spatial Sensor Lab**: the phone as a live 3D model that turns as you turn it, the magnetic field as an arrow with its strength in µT, and a field anomaly monitor: set a baseline, move the phone near an object, and see how far the field moved, with the last 30 seconds as a graph ([below](#spatial-sensor-lab)).
  - AI Lab, Background Downloads and Crash Test.

## What it shows

| Section | Values | Needs |
|---------|--------|-------|
| **Network** | Online or offline and why, Wi-Fi, cellular or wired, metered, Low Data Mode, IPv4/IPv6/DNS, this phone's own IP addresses (Wi-Fi, cellular, VPN, with Tailscale named), whether a Wi-Fi-only download would start, a log of every change | Nothing |
| **Bluetooth** | On, off, not allowed, not supported | Bluetooth permission (asked from a button) |
| **Battery & Power** | Level and charging state, Low Power Mode, thermal state, screen brightness, whether Forge's heavy-work rule would let a big download run, the drain in percent an hour on battery (after 10 minutes) with a sparkline, and every heat change since launch. Drain and heat keep counting while another tab or a hosted app is open, so they show what an AI Lab run or a big transfer cost | Nothing |
| **Audio** | Where sound is going (speaker, headphones, Bluetooth, AirPlay, CarPlay), the microphone in use, the volume, whether another app is playing, and every interruption and route change since launch, including those a hosted app (Soundscape) gets while it plays | Nothing |
| **Storage & Memory** | Free and total storage, memory the app uses, memory it has left before iOS ends it, device RAM | Nothing |
| **Motion** | Accelerometer, gyroscope and magnetometer (each axis, magnitude, peak, measured rate, sparkline), roll, pitch, yaw, tilt, movement without gravity, compass calibration | Nothing |
| **Compass** | A turning dial. True north from Core Location when location is allowed, magnetic north from device motion otherwise | Nothing (location for true north) |
| **Barometer** | Air pressure with its range and a sparkline, height change since it started | Motion & Fitness permission |
| **Location** | Latitude, longitude, accuracy, altitude, speed, course, floor, distance travelled, top speed, Precise Location, a link to Maps | Location permission, while in use |
| **Device** | Model identifier, iOS version, cores, screen size, uptime | Nothing |
| **Spatial Sensor Lab** | A 3D phone following the fused attitude, seen from a fixed point south of it; pitch, roll, yaw, gravity, movement, rate; the calibrated magnetic field as a 3D arrow, its strength, dip and calibration; a baseline averaged over half a second and the anomaly from it (vector change, strength change, peak, 30-second graph) | Nothing |
| **AI Lab** | Ready-made experiments with Apple's on-device model (short answer, following instructions, summarising, structured output twice, through Apple's `@Generable` macro and through `StructuredShape`, a no-macro stand-in that builds on Linux, a long answer to stop part-way) and a prompt of your own. Each run records time to first text, total time, an estimated speed, how many pieces it streamed in, and the app's memory and thermal state before and after ([plan, phase A0](../../docs/research/esim-ai-lab-plan.md)) | iOS 26 with Apple Intelligence on |
| **Bonsai Harness** | Give a downloaded Bonsai model (Bonsai on iPhone) a Python task: it writes, runs and fixes files in a Python Lab project, one grammar-constrained tool call at a time, and the harness marks the task Completed only when Python's own output proves it. Three ladder tasks (hello, one test, one fix) or your own; each step's thought, call and result; Stop; the transcript to share; the project's files. Every step is kept in the project's journal ([plan](docs/bonsai-harness-plan.md)) | A downloaded Bonsai model |
| **Background Downloads** | A download that keeps going after you leave the app, through Forge's background transfers | Nothing |
| **What iOS Keeps from Apps** | Wi-Fi name (paid team), the phone's own name (Apple approval), carrier, signal strength, the light sensor, MAC addresses, background sensors | |
| **Crash Test** | For developers: crashes the app on purpose with a Swift force unwrap or a Rust panic, to check `forge device crashes --pull` down to the file and line ([forge-device.md](../../docs/forge-device.md#crash-reports)) | |

Nothing asks for a permission until you tap its button. Everything stops when the app goes to the background, and nothing leaves the phone. All of it works with a free Apple account: the app has no entitlements.

## The other apps (Apps tab)

The **Apps** tab, the first thing Device Lab shows, opens the other example
apps, each the full app: Weather, Checklist, Pocket Library, Good Dog, Commonplace, Relay (with Chat's
messaging, merged into it), FieldKit, Urdu, Naturally and the ForgeUI Showcase. **Open at Launch** on that screen makes Device Lab start
in one of them (the `-launchApp weather` launch argument does the same).
One app on the phone holds all of them. (Counter isn't hosted.) The top row,
**This Build**, shows the installed version and build number; every
`forge run ios` stamps a new build number, so it is how a phone sitting checks
that the new build arrived.

### The workshop

An app opens full screen, laid out exactly as when it is launched on its own:
its own status bar, safe areas and gestures. A small round button on its
trailing edge (**Workshop**) is all Device Lab adds. Its menu:

| Item | What it does |
|------|--------------|
| **Back to Device Lab** | Closes the app (its core keeps running, as in the background). VoiceOver's two-finger scrub does the same once the app has nothing left to dismiss |
| **Switch App** | Opens another hosted app in this one's place, without going back to the list. The workshop choices below carry over |
| **Appearance** | Light, dark, or match the iPhone, without changing the phone's setting |
| **Text Size** | Default, the largest ordinary size, Accessibility 3 or 5, without changing the phone's setting |
| **Screen Size** | The app in a 375 × 667 point frame, the smallest iPhone iOS 17 runs on (iPhone SE), centred on this phone. An approximation: the safe areas are this phone's |
| **Move This Button** | Top, middle or bottom of the edge, off whatever it covers |

The choices change only how the app is shown, never its data, and last until
Device Lab quits. Launch arguments set them for UI tests and screenshots:
`-launchApp <app>` opens an app, `-ForgeAppearance dark|light`,
`-WorkshopTextSize large|xxxl|ax3|ax5`, `-WorkshopScreen compact`, and
`-ForgeTemporaryData` starts every app on empty throwaway data.

### The screenshot catalogue

`uitests/catalogue.toml` shoots every app's first screen, light and dark, on
throwaway data: the "before" of every redesign. Run it with the phone on the
cable, look at the screens, and approve them as baselines
([visual-regression.md](../../docs/visual-regression.md)):

```bash
forge ui run examples/device-lab/uitests/catalogue.toml --visual record
forge ui accept <run>    # after looking at them
```

After a change, `forge ui run examples/device-lab/uitests/catalogue.toml`
says which screens changed and where.

Each app stays in its own folder and is updated there; Device Lab uses it as
it is:

- **Rust:** `rust/Cargo.toml` depends on each app's own crate, which names its
  library after the app (`weather_core`, `relay_core`, ...), and `rust/src/lib.rs`
  links it with `use <library> as _;`.
- **Swift:** `Package.swift` depends on each app's own package
  (`.package(path: "../relay")`) and its app product (`RelayApp`). Each app names
  its Swift modules after itself in its `forge.toml` (`[swift] bindings_module
  = "RelayFFI"`, `app_module = "RelayApp"`), so no two modules share a name. An
  app's package links its Rust library only from its app, launcher and test
  targets, so here only Device Lab's library is linked, which holds them all.
- `Sources/AppHost/AppsView.swift` lists the apps, opens them, and installs
  their native services and lifecycle (`EmbeddedApps.install()`).

Hosting is optional ([roadmap](../../docs/roadmap.md#tooling-freeze-and-reuse-rules), rule 6): a new
app is published and tested on its own, and joins here only for a combined
phone sitting or when asked. To add an app with a Rust crate:

1. Make the app hostable, in its own folder (it still builds on its own):
   - `rust/Cargo.toml`: name the library after the app (`[lib] name =
     "<library>"`, `crate-type = ["staticlib", "lib"]`), and call
     `uniffi::setup_scaffolding!()` with no argument in `lib.rs`.
   - `forge.toml`: add `[swift]` with `bindings_module = "<App>FFI"` and
     `app_module = "<App>App"`.
   - `Package.swift`: use those names for the bindings and app targets, call
     the C module `<library>FFI`, give every target in a product the app's
     name (not `AppHost...`), link the Rust library (`rustLibrary`) from the
     app, launcher and test targets only, and put the bindings target in the
     app's library product. Change `import ForgeFFI` and `import AppHost` in
     the app's Swift to the new names.
   - `forge bindings generate --project examples/<app>`.
2. Add the crate to this app's `rust/Cargo.toml` and a `use <library> as _;`
   line to `rust/src/lib.rs`.
3. Add `.package(path: "../<app>")` to `Package.swift` and the app's product
   to `AppHost`.
4. Add a case to `EmbeddedApp` in `Sources/AppHost/AppsView.swift` (and, if the
   app has native services or a ForgeKit host, to `EmbeddedApps.install()`),
   plus any permission text or background mode it needs to `forge.toml`.

An app without Rust (like the ForgeUI Showcase) needs only steps 3 and 4.

## Spatial Sensor Lab

A self-contained instrument in Tools, built on Core Motion directly rather than on a Forge service: one `CMMotionManager`, one device-motion stream at 60 Hz referenced to magnetic north (`.xMagneticNorthZVertical`), running only while the screen is open and the app is in the foreground. The This iPhone tab's sensors are stopped on the Tools tab, so it is the only motion stream running.

- `SensorLabMotion.swift` copies each `CMDeviceMotion` (attitude matrix, roll, pitch, yaw, gravity, user acceleration, calibrated field and its accuracy) into a plain sample.
- `SensorLabModel.swift` (Foundation and Observation only) holds the baseline and the anomaly: the field turned into world axes with the attitude, so turning the phone doesn't count as a change, minus the baseline's.
- `SensorLabScene.swift` is the 3D view's geometry, and `SensorLabView.swift` draws it with a SwiftUI `Canvas`: no SceneKit, RealityKit or Metal shaders, so it builds with xtool on Linux.

It shows field anomaly, never distance or identity. A magnetometer measures the field at the phone; the same change can come from a small magnet close by or a big one further off, so the screen says how much the field moved and nothing about what moved it. A neodymium magnet shows strongly; speakers and magnetic accessories often; steel a little; aluminium and copper not at all.

Not yet: a recording mode for scans, raw accelerometer, gyroscope and magnetometer panels (the This iPhone tab's Motion screen has those), and a calibration and sensor-health screen.

## How it is built

```
examples/device-lab/
  forge.toml               permission strings for location, motion, Bluetooth; team_type = "free"
  rust/src/lib.rs          native_services!(connectivity, device_status, motion, location, bluetooth);
                           DeviceLab: what runs when, LabSnapshot for the UI
  rust/src/stats.rs        rolling histories, measured rates, peaks, tilt, distance, change log
  Generated/ForgeFFI/      bindings plus the five optional Swift adapters (copied because lib.rs names them)
  Sources/AppHost/         SwiftUI: ContentView (the tabs), AppsView, DashboardView (This iPhone),
                           ToolsView, WorkshopView, LabModel, MotionView, CompassView, LimitsView,
                           SensorLab* (Spatial Sensor Lab: Core Motion straight from Swift)
  Resources/AppIcon.png    the home-screen icon
```

**Forge owns** reading the platform: `NWPathMonitor`, `UIDevice` and `ProcessInfo`, Core Motion, Core Location and Core Bluetooth, behind Rust traits, with permissions through the one `NativePermission` interface.

**Rust (this app) owns** when each source runs (only while on screen, and only once its permission is granted), and what the app makes of the values: sparkline histories, update rates measured from sensor timestamps, peaks and ranges, tilt from gravity, distance travelled over accurate fixes, and the connectivity log. All tested against Forge's fakes with `cargo test` on Linux.

**SwiftUI owns** the screens and every word on them. `LabModel` copies `DeviceLab.snapshot()` ten times a second while the app is open; a snapshot is a memory copy.

## Build and run from Linux

The same as any Forge app ([docs/linux-setup.md](../../docs/linux-setup.md)):

```bash
cd examples/device-lab
forge doctor
forge run ios
```

A free Apple account allows three sideloaded apps at a time, so you may need to remove one first.

`cargo test --manifest-path rust/Cargo.toml` runs the Rust tests anywhere. On a Mac, `forge test --project .` also runs the XCTest suite, where connectivity and device status are real and the iOS-only sensors report that they are unavailable.

## Checking it on an iPhone

Device Lab has been on the phone since 14 November 2026, as the app every phone sitting uses. To check the dashboard itself (This iPhone tab, AI Lab and Crash Test in Tools):

1. Network: toggle Airplane Mode and Wi-Fi from Control Center; each change appears in Recent Changes, and Large Downloads says "Would wait for Wi-Fi" on cellular.
2. Bluetooth: tap Allow, then toggle Bluetooth in Control Center.
3. Motion: shake the phone and watch the peaks; rates read about 20 Hz.
4. Compass: turn around; allow location and North changes to True.
5. Barometer: allow Motion & Fitness; lift the phone from floor to ceiling for a height change of about 2 m.
6. Location: allow, walk 100 m, and check Travelled.
7. Battery: plug in and out; turn on Low Power Mode.
8. AI Lab: Status reads Ready. Run each experiment once: Short Answer streams in pieces with a first-text time; Follow Instructions gives exactly five numbered lines; Structured Output and Structured, No Macro each show the fields filling in, then the dish, minutes, ingredients and steps; stop one part-way and the partial answer stays; start Long Answer and tap Stop part-way, and the run ends as Stopped within a second. With Apple Intelligence turned off in Settings, Check Again shows why it is unavailable.
9. Spatial Sensor Lab (Tools): lay the phone flat and the model lies flat, screen up. Turn it on the table and the model turns the same way against its compass ring (N at the back, so face north to see it from behind); lift its top and the model lifts its top; turn it over and the model shows its back with the camera. Once calibration reads Medium or High, the field is 25 to 65 µT. Set Baseline away from metal, then bring a magnet (a MagSafe charger or a fridge magnet) near: the anomaly rises and the badge reads Clear or Strong disturbance; take it away and it settles back under 3 µT. Turning the phone slowly without a magnet should stay under about 5 µT. Leaving the screen stops the stream.
10. Crash Test: tap Crash in Swift, then on the laptop `forge device crashes --app AppHostXtool --pull crashes/ --limit 1 --project examples/device-lab`. Expect `swift_runtime_error` with the culprit at `LabModel.swift`. Open the app again, tap Crash in Rust, pull again: `rust_panic`, culprit `on_reading` in `rust/src/crash_test.rs` (Rust aborts at the callback that couldn't unwind; `forge device logs --grep panic` running at the time shows the message and line of the `expect`).
