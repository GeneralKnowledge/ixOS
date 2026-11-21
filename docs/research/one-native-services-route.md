# One route for native services

**Status:** design note, 2026-11-21. This is the investigation that Phase 4 of the [everyday capabilities plan](everyday-capabilities.md#phase-4-one-native-services-route) asks for before any commitment. Reuben asked for it the same day ("Go for it"), after the ForgeKit review found two hand-kept copies of the services' Swift.

**Answer: yes, with one change to Phase 4's shape.** Optional services should not become Cargo features of one `forge-native` crate. Each one becomes its own small crate and its own ForgeKit product. Step 1, one source for the Swift that both routes ship, is small and is worth doing now, whatever happens to the rest.

## Where things stand

There are two routes to the same services ([native-services.md](../native-services.md#how-it-fits-together)):

| Route | Apps | Rust | Swift |
|---|---|---|---|
| Shared crate | the starter, Good Dog (and Device Lab, which hosts Good Dog) | `pub use forge_native as native;` | ForgeKit's `ForgeNative` product |
| Macro | Checklist, Commonplace, FieldKit, Pocket Library, Relay, Sketchbook, Soundscape, Sums, Urdu Naturally, Weather, Device Lab's own services | `forge_services::native_services!(…)` | copied into `Generated/ForgeFFI` by `generate-bindings.sh` |

- **The core adapters exist twice.** That's seven files: Keychain, files, notifications, permissions, sharing, deep links and `ForgeServices.swift`. They live in `crates/forge-services/swift/` and again in `forgekit/Sources/ForgeNative/`. Five of them differ only in their header and an `#if canImport(Darwin)` wrapper. `ForgeServices.swift` and the deep links file have real differences: the `optional:` installers, and the version hint. Nothing checks that the copies stay in step. Today's logging fix (#363) had to edit both by hand.
- **Optional services exist on the macro route only.** All 11 of them: ai, audio, background, biometrics, bluetooth, connectivity, device status, http, location, motion and transfers. An app made by `forge init` outside this repository has no way to get `http` or `location` from the ForgeKit release alone. Phase 4 calls this the motivating case, A35.

## The questions Phase 4 asked

**1. Can optional services be Cargo features of `forge-native`? No.**
- The Swift in `ForgeNative` is generated once per ForgeKit release, by `scripts/forgekit.sh`, from one build of the crate. If the release is generated with every feature on, the Swift calls Rust functions (and UniFFI's checksum functions, which run at first use) that an app with the feature off doesn't have. The app then fails to link.
- If the release is generated with features off, no app can use them.
- SwiftPM has no per-feature compilation, so one `ForgeNative` product can't adapt to the app.

**2. What does an unused adapter cost?**
- With every adapter in one product, every app links Core Location, Core Motion, Core Bluetooth, Local Authentication and the microphone APIs, whether it uses them or not.
- App Store processing flags binaries whose code calls a protected API without the matching purpose string in Info.plist (ITMS-90683, the usual case being location). This is from Apple's documented review behaviour, not something we have hit. It matters now that the [first App Store release](../first-app-store-release.md) is being planned.
- The macro route avoids this today by copying only the adapters an app selects. The new route has to keep that property.

**3. How does an app select a service?**
- Through its own crate and its own product. The app's Rust crate depends on `forge-native-location` and re-exports it. Its `Package.swift` adds the `ForgeNativeLocation` product.
- At launch it passes the installer: `ForgeServices.install(optional: [ForgeNativeLocation.service])`. `ForgeNative`'s `install(optional:)` was already written for this.
- UniFFI external types across components already work: `ForgeApp` and `ForgeNative` are two components today.
- **Staying in step with `forge.toml` permissions** (review P1): `forge build` already warns when a selected service lacks its permission text. On the new route it reads the selection from the app's Cargo dependencies instead of `ForgeServices+Selected.swift`.

**4. Can the copying route retire app by app?** Yes.
- An app moves when its crate swaps `native_services!(location)` for the dependency and its Swift swaps `ForgeFFI` names for imports. No app depends on another app's route.
- `generate-bindings.sh` keeps copying for the apps that haven't moved, and the copying is deleted with the last one.
- A parity rule, `forge-native`, lists the apps that haven't moved, and each wave shrinks it. The rule only shrinks, as usual.

**5. Is the result a clean ForgeKit release boundary?** Yes.
- Each wave that adds products is a ForgeKit minor release (0.8, 0.9, and so on).
- The new drift check in `check-template-sync.sh` (#363) fails while forge-app's version equals the released one and `forgekit/` has changed since that release. So a change to ForgeKit has to bump the version, and the bump is the reminder to release. It doesn't stop a bumped version going unreleased.

## The plan

**Step 1. One source for the core adapters (small, do now).**
- `scripts/forgekit.sh generate` writes `forgekit/Sources/ForgeNative/ForgeServices*.swift` from `crates/forge-services/swift/`, with ForgeKit's header and the Darwin guard.
- The two real differences move into the source behind a small `#if FORGE_NATIVE` (a compilation condition set by ForgeKit's `Package.swift`):
  - the `optional:` installers;
  - the version hint text.
- `scripts/forgekit.sh check`, which already runs in CI, fails when the copies differ, the same way it checks the bindings.
- After this, a fix to an adapter is made once.
- **Phone:** none. It's a Swift compile in the combined build, with the starter and Good Dog as the users.

**Step 2. The first two services: http and location.**
- New crates `crates/forge-native-http` and `crates/forge-native-location`. Their Rust items move out of `native_services!`, which re-exports them so macro apps don't change.
- New ForgeKit products `ForgeNativeHttp` and `ForgeNativeLocation`, generated by `forgekit.sh` like `ForgeNative`.
- Weather moves (http and location), then Soundscape (http): the smallest needs, as Phase 4 planned.
- **Acceptance:** a fresh `forge init` app outside the checkout uses `http` and `location` with only the ForgeKit release. `scripts/check-fresh-app.sh` gains that case.
- **Release:** ForgeKit 0.8.
- **Phone:** Weather finds your location, and Soundscape streams a song, both in Device Lab.

**Step 3. Waves for the other nine services.**
- One wave per group of apps, in this order:
  - FieldKit (location);
  - Commonplace (background, connectivity);
  - Pocket Library (ai);
  - Relay (audio, biometrics, ai, background);
  - Device Lab (bluetooth, motion, device status, transfers);
  - Checklist, Sketchbook, Sums and Urdu Naturally, which use only the core services and move to `ForgeNative` alone.
- After that, the macro and the copying are removed.
- **Rule:** `forge-native`, with each wave deleting its lag entries.

**Not in scope:** Android adapters, and splitting ForgeKit into several packages (these are products inside the one package).

## Decided (Reuben, 2026-11-21)

Reuben said to go with the recommendations:

1. **Grouping.** Every optional service gets its own crate and product, including the ones with no privacy prompt. The core `ForgeNative` stays small.
2. **When.** Step 1 goes into the PR after #363. Steps 2 and 3 start the week after the ForgeKit 0.7 release.
