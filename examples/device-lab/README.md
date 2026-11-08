# Device Lab

A live view of everything an iPhone tells an app about itself: network, Bluetooth, battery and temperature, storage and memory, the motion sensors, a compass, the barometer, location, and what the device is. It also lists what iOS keeps from apps, and why.

It exists to prove Forge's optional native services ([docs/native-services.md](../../docs/native-services.md#optional-services)). The device capabilities are the framework's; this app is their consumer.

## What it shows

| Section | Values | Needs |
|---------|--------|-------|
| **Network** | Online or offline and why, Wi-Fi, cellular or wired, metered, Low Data Mode, IPv4/IPv6/DNS, whether a Wi-Fi-only download would start, a log of every change | Nothing |
| **Bluetooth** | On, off, not allowed, not supported | Bluetooth permission (asked from a button) |
| **Battery & Power** | Level and charging state, Low Power Mode, thermal state, screen brightness, whether Forge's heavy-work rule would let a big download run | Nothing |
| **Storage & Memory** | Free and total storage, memory the app uses, memory it has left before iOS ends it, device RAM | Nothing |
| **Motion** | Accelerometer, gyroscope and magnetometer (each axis, magnitude, peak, measured rate, sparkline), roll, pitch, yaw, tilt, movement without gravity, compass calibration | Nothing |
| **Compass** | A turning dial. True north from Core Location when location is allowed, magnetic north from device motion otherwise | Nothing (location for true north) |
| **Barometer** | Air pressure with its range and a sparkline, height change since it started | Motion & Fitness permission |
| **Location** | Latitude, longitude, accuracy, altitude, speed, course, floor, distance travelled, top speed, Precise Location, a link to Maps | Location permission, while in use |
| **Device** | Model identifier, iOS version, cores, screen size, uptime | Nothing |
| **What iOS Keeps from Apps** | Wi-Fi name (paid team), the phone's own name (Apple approval), carrier, signal strength, the light sensor, MAC addresses, background sensors | |
| **Crash Test** | For developers: crashes the app on purpose with a Swift force unwrap or a Rust panic, to check `forge device crashes --pull` down to the file and line ([forge-device.md](../../docs/forge-device.md#crash-reports)) | |

Nothing asks for a permission until you tap its button. Everything stops when the app goes to the background, and nothing leaves the phone. All of it works with a free Apple account: the app has no entitlements.

## How it is built

```
examples/device-lab/
  forge.toml               permission strings for location, motion, Bluetooth; team_type = "free"
  rust/src/lib.rs          native_services!(connectivity, device_status, motion, location, bluetooth);
                           DeviceLab: what runs when, LabSnapshot for the UI
  rust/src/stats.rs        rolling histories, measured rates, peaks, tilt, distance, change log
  Generated/ForgeFFI/      bindings plus the five optional Swift adapters (copied because lib.rs names them)
  Sources/AppHost/         SwiftUI: LabModel, DashboardView, MotionView, CompassView, LimitsView
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

Not yet run on a device. When it is, check:

1. Network: toggle Airplane Mode and Wi-Fi from Control Center; each change appears in Recent Changes, and Large Downloads says "Would wait for Wi-Fi" on cellular.
2. Bluetooth: tap Allow, then toggle Bluetooth in Control Center.
3. Motion: shake the phone and watch the peaks; rates read about 20 Hz.
4. Compass: turn around; allow location and North changes to True.
5. Barometer: allow Motion & Fitness; lift the phone from floor to ceiling for a height change of about 2 m.
6. Location: allow, walk 100 m, and check Travelled.
7. Battery: plug in and out; turn on Low Power Mode.
8. Crash Test: tap Crash in Swift, then on the laptop `forge device crashes --app AppHostXtool --pull crashes/ --limit 1 --project examples/device-lab`. Expect `swift_runtime_error` with the culprit at `LabModel.swift`. Open the app again, tap Crash in Rust, pull again: `rust_panic`, culprit `on_reading` in `rust/src/crash_test.rs` (Rust aborts at the callback that couldn't unwind; `forge device logs --grep panic` running at the time shows the message and line of the `expect`).
