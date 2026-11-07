# Trial: an existing Bevy game on an iPhone, built on Linux

Plan, 2026-11-07. Kataster has been built for iPhone on Linux (see [Results so far](#results-so-far)); the trial files are in [`examples/kataster-ios/`](../examples/kataster-ios/). This is a compatibility test: take a small open-source Bevy game, change it as little as possible, and get it running on a physical iPhone from Linux with Forge and xtool. What breaks tells us what ixOS is missing for real-time Rust apps. We do not build a Bevy framework. Anything that turns out to be reusable goes into ixOS as a general capability (see [architecture.md](architecture.md#framework-principles)).

## The game: Kataster

[Kataster](https://github.com/Bobox214/Kataster) is a single-screen asteroids shooter by Boris Boutillier. It is about 1,200 lines and has menus, a HUD, game states, pause, physics (`avian2d` 0.4), input actions (`leafwing-input-manager` 0.18), GPU particles (`bevy_hanabi` 0.17, compute shaders), a WGSL background shader and sound. It targets Bevy 0.17.2 (last commit d0a67e7, 2025-10-22).

We keep its name and look, credit its author, and do not copy it into this repository. A script fetches the pinned commit and applies our small patch.

### Licences

| Part | Licence | Source |
|---|---|---|
| Code | MIT OR Apache-2.0 | `Cargo.toml`, `LICENSE.txt` (which points to `docs/LICENSE-*` files the repo doesn't have) |
| Sprites, `kenvector_future.ttf`, sounds, smoke particles | Kenney assets. `assets/CREDITS.md` names Kenney but gives no licence; Kenney publishes these packs as CC0 | To confirm against the Space Shooter Redux and Smoke Particles pack pages before we publish anything |
| `FiraSans-Bold.ttf` | SIL OFL 1.1 | `assets/Fira-LICENSE` |
| `background.wgsl` | Kataster's own shader, "inspired by" Star Nest by Pablo Roman Andrioli (Shadertoy XlfGRj) | The original states MIT in its source; to confirm on the Shadertoy page |

All of this allows a private test build. We will not publish an IPA or screenshots that misattribute the game. If we ever do, we'll ship the credits and the OFL text with it.

### Touch control plugin

Kataster is keyboard only: WASD or arrows to fly, Space to fire, Enter and arrows in menus, Esc to pause. Its menu buttons are drawn but not clickable, because selection is keyboard driven.

| Plugin | Version for Bevy 0.17 | Notes |
|---|---|---|
| [virtual_joystick](https://github.com/SergioRibera/virtual_joystick) | **2.6.0** (`bevy ^0.17`) | MIT/Apache. 2.7.x targets 0.18; 2.8.0 (latest) targets 0.19; nothing for 0.20 yet. **Chosen** |
| [bevy_touch_stick](https://github.com/johanhelsing/bevy_touch_stick) | none | Release 0.2.0 and main both target Bevy 0.13. Not usable |

Checked on crates.io and both repositories, 2026-11-07.

### Bevy version

We stay on 0.17 for the first try, because Kataster and all its plugins already have matching releases. Moving to a newer Bevy means bumping `avian2d`, `leafwing-input-manager`, `bevy_hanabi` and the joystick together, which is a port of someone else's game. If 0.17 hits an iOS problem that a newer Bevy fixes, we move everything to 0.19 (the newest release the joystick supports). Reuben is fine with upgrading when it helps.

## How Bevy runs on iOS

These facts were checked in the Bevy v0.17.2 and v0.20.1 sources:

- **Entry point.** On iOS a Bevy game is an ordinary Rust **executable**: `fn main()` builds the `App`, and winit's event loop calls `UIApplicationMain` itself and never returns. `#[bevy_main]` only adds an `android_main` symbol. Bevy's own `examples/mobile` builds `--bin` for `aarch64-apple-ios` and copies the binary into the `.app` as its executable.
- **Rendering.** wgpu uses Metal through objc2. Shaders are compiled from WGSL at run time by naga, so no `.metal` precompile is needed (xtool can't do that on Linux).
- **Frameworks.** UIKit, Metal, QuartzCore, CoreGraphics, Foundation, CoreFoundation, AudioToolbox, CoreAudio and AVFAudio come in through `#[link(kind = "framework")]` attributes. rustc links them automatically for an executable.
- **Audio.** In Kataster's lock file (Bevy 0.17), `rodio` 0.20 uses `cpal` 0.15, which uses `coreaudio-sys` 0.2.17. That crate runs **bindgen** against the SDK headers at build time. It runs `xcrun` unless `COREAUDIO_SDK_PATH` is set, and it needs libclang. Bevy 0.20 no longer has this problem: its `cpal` 0.17 uses objc2 bindings.
- **C code.** `blake3` compiles a NEON C file with `cc`, which needs `SDKROOT` and clang, like our other iOS builds.
- **Gamepads.** gilrs has no iOS backend, so Bevy sees no controllers on iOS. Game controllers would need a native GameController bridge later.
- **Lifecycle.** Bevy emits `AppLifecycle` (`Idle`, `Running`, `WillSuspend`, `Suspended`, `WillResume`). `WinitSettings::mobile()` stops updating while backgrounded. Kataster sets neither.
- **Touch.** It arrives as `TouchInput` messages and the `Touches` resource. Bevy UI `Interaction` also responds to touch.
- **Orientation and screen.** Supported orientations come from Info.plist. `Window` has `prefers_status_bar_hidden`, `prefers_home_indicator_hidden` and `preferred_screen_edges_deferring_system_gestures`. There is no safe-area API.
- **Assets.** `AssetServer` reads from `BEVY_ASSET_ROOT`, else `CARGO_MANIFEST_DIR`, else the executable's folder. On a phone that is `Kataster.app/assets/`.
- **Logs.** `bevy_log` writes to os_log on iOS (`tracing-oslog`).

## Building it with Forge on Linux

Forge apps today are a SwiftUI host plus a Rust **static library** called through UniFFI, built by xtool from a SwiftPM package. A Bevy game doesn't fit that shape:

- winit insists on creating the `UIApplication` itself, so it can't run inside SwiftUI's.
- The game is a binary crate with no library target.

There are two ways to fit it in.

### Route 1: Rust executable app (recommended)

Build the game as Bevy intends, as an iOS executable. Then let Forge assemble the bundle and xtool sign and install it:

1. `cargo build --target aarch64-apple-ios --bin kataster`, linked with the clang and `ld64.lld` that xtool's Darwin SDK already ships. The environment needs `SDKROOT`, `CC`, the linker, `-fuse-ld=lld`, and `COREAUDIO_SDK_PATH=$SDKROOT` plus libclang for bindgen.
2. Assemble `Kataster.app` from three parts:
   - the binary;
   - `Info.plist`, generated from `forge.toml` by the same code that writes it for SwiftUI apps today;
   - `assets/`.

   Info.plist needs `UILaunchScreen`, or iOS runs the app letterboxed. It also needs landscape-only orientations, `UIRequiresFullScreen` and a hidden status bar.
3. `xtool install Kataster.app`, which signs with the free Apple ID and installs over USB. This step is the main open question; see the risks below.

This route needs no change to the game's entry point, and it works for any winit game (Bevy, macroquad, a plain wgpu app). It is also the general capability ixOS lacks: shipping a Rust app that has no Swift UI.

### Route 2: Swift game host (fallback)

A SwiftPM package whose xtool library target has a plain `@main struct GameHost { static func main() }`. That function calls an exported Rust function, which runs the game's `main`. It must not be a SwiftUI `App`, because winit would find a `UIApplication` already running.

This needs the game to gain a library target and an `extern "C"` entry function, which is more change to the game. It also needs every framework listed in Swift. Its advantage is that it is exactly the build xtool already does for our apps. Assets would go in through `xtool.yml` `resources:` (copied to the bundle root) or `BEVY_ASSET_ROOT`.

We try route 1 first and fall back to route 2 if xtool can't sign a bundle it didn't build.

## Changes to the game

The patch adds one file and touches two lines. It is kept in `examples/kataster-ios/patch/` as a patch against d0a67e7.

- **`Cargo.toml`:** add `virtual_joystick = "2.6"`.
- **`src/main.rs`:** add `mod mobile;` and `app.add_plugins(mobile::MobilePlugin)`, compiled only for iOS.
- **`src/mobile.rs`** (new, iOS only):
  - A floating virtual joystick on the left half of the screen. Up means thrust; left and right mean rotate. It presses Kataster's own `PlayerAction`s through leafwing's `ActionState` (in leafwing's `ManualControl` set), so the game logic is unchanged.
  - Hold anywhere on the right half to fire; tap the top-right corner to pause or resume. These read raw `Touches` rather than Bevy UI buttons, because Bevy UI follows a single pointer and the joystick already holds one finger.
  - Menus: tapping a menu item selects it and presses `MenuAction::Accept`. This works by reading `Interaction` on the menu buttons.
  - Screen fit: the arena is a fixed 1280 x 800, larger than any phone in points. Set the camera's projection to `ScalingMode::AutoMin { 1280, 800 }` so the whole arena shows. The system only changes the camera Kataster spawns.
  - `WinitSettings::mobile()` and an `AppLifecycle` handler that moves the game to its own `Paused` state when the app is suspended.
  - The frame rate is logged every five seconds at warn level (`[trial] fps ...`), along with lifecycle changes, so `forge device logs` shows them.
  - The **Exit** menu item still quits the app. iOS apps shouldn't quit themselves, but stopping it would mean changing Kataster's menus, which isn't worth it for a trial.

Debug builds also enable Bevy Remote (an HTTP server) and the physics debug overlay. We build with `--release`, which drops both and is needed for frame rate anyway.

## Testing and diagnostics

Reuse Forge Device and the UI testing work. Don't build anything game-specific.

| Need | What we use | Gap |
|---|---|---|
| Logs | `forge device logs --process kataster` (Bevy writes os_log) | Our syslog relay drops Info level, so only Bevy warnings and errors show. Fine for a first run; to fix, route Bevy's tracing through `forge_runtime::logging` |
| Crashes | `forge device crashes --app kataster --pull crashes/` | None expected |
| Frame rate | Bevy's `FrameTimeDiagnosticsPlugin` logged at warn level once a second, in the trial patch | No Forge performance channel yet. The `forge-devtools` live connection could carry FPS, memory and thermal state once a Rust-only app can link it |
| Thermal and memory | Not available: the Device Lab's `device_status` service needs the Swift host | Needs a Rust-only path to these signals (`ProcessInfo` via objc2) |
| Automated play | WebDriverAgent (the "Automate testing on a real iPhone" work) can launch, background, foreground and rotate the app | Bevy draws everything itself and exposes no iOS accessibility tree, so WDA can't find buttons by identifier. Don't fall back to coordinates. Use in-game test commands (press action X, jump to state Y) through `forge-devtools` later |

## Trial steps

1. **Build on Reuben's laptop**, run by me through Remote Control. Fetch Kataster d0a67e7, apply the patch, and build the iOS executable with route 1's environment. Record every failure. No install.
2. **Assemble and sign** `Kataster.app`. Check that `xtool install` accepts it, or fall back to route 2.
3. **Install on Cati**, only after Reuben agrees. Cati is at the free account's 3-app limit and is busy with the AI test, so something has to be removed first.
4. **On-phone checks** (Reuben):
   - the menu shows, and tapping Play starts a game;
   - the joystick and fire button work;
   - sound plays;
   - the whole arena is visible in landscape;
   - after backgrounding for 30 seconds the game comes back paused;
   - no crash after 5 minutes of play;
   - roughly what frame rate the log shows.
5. **Record the results** in the capability ledger and turn each gap into an ixOS item.

## Results so far

**Build, 2026-11-07, Reuben's laptop:** Kataster d0a67e7 built **unmodified** as an iOS executable. Setup was xtool 1.24.0, the iPhoneOS 26.0 SDK, Rust 1.97.0, the Swift toolchain's clang 17 and libclang 18.

- **The binary:** Mach-O arm64, platform ios, minos 17.0. It links only system frameworks: UIKit, Foundation, CoreFoundation, AudioToolbox, CoreAudio, QuartzCore, Metal and CoreGraphics, plus libobjc, libSystem and libiconv.
- **Time and size:** 9 minutes from cold (94 CPU-minutes). The binary is 147 MB, or 85 MB with symbols stripped; the assets are 1.1 MB.
- **The iOS link worked first time** with `-fuse-ld=lld --ld-path=<sdk bundle>/toolset/bin/orig/ld64.lld`. The `toolset/bin/ld64.lld` beside it is xtool's wrapper script. bindgen for `coreaudio-sys` worked with `COREAUDIO_SDK_PATH` and `BINDGEN_EXTRA_CLANG_ARGS`. `examples/kataster-ios/build.sh` records the full environment.
- **Two host fixes, no game changes:**
  - Kataster's `.cargo/config.toml` (Bevy's fast-build template) sets `/usr/bin/clang` as the Linux linker. We override it with `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER`.
  - The laptop has no default rustup toolchain, so we set `RUSTUP_TOOLCHAIN`.
- **lld warns:** "This version of lld does not support linking for platform iOS. Proceed at your own risk." It is the same lld that xtool uses for every Forge app, but only a launch on the phone will prove the binary works.
- **Signing:** xtool 1.24.0 has no sign-only command for a bundle it didn't build. `xtool install <app>` signs and installs in one step. `xtool dev build --sign --ipa` signs locally, but only for SwiftPM projects. So we sign at install time for now. A local signing step (a stub SwiftPM wrapper, or rcodesign with xtool's certificate) is a gap.

Not yet run on the iPhone.

## Expected gaps in ixOS

1. **Rust-executable apps.** forge-cli and `build-rust-ios.sh` build only static libraries, and every template has a Swift host. We need a way to build a Rust binary for iOS on Linux (linker setup from xtool's SDK) and package it with a generated Info.plist and a resource folder. This is general, not Bevy-specific: any winit or wgpu app needs it.
2. **Signing a prebuilt bundle.** Today xtool signs only what it builds through `forge build ios`. Installing an `.app` Forge assembled itself also unblocks the over-the-air plan in Forge Device M3.
3. **Game-oriented Info.plist settings** in `forge.toml [ios]`: orientations, full screen, status bar, launch screen.
4. **bindgen on Linux.** Some crates (here `coreaudio-sys`) need `COREAUDIO_SDK_PATH` and libclang. `forge doctor` should check for libclang.
5. **Diagnostics without Swift:** Info-level logs, frame time, memory and thermal state for apps with no Swift adapter.
6. **Testing apps that draw their own UI.** This needs an in-app command channel, not accessibility identifiers.
7. **Touch controls.** We reuse `virtual_joystick` now. A reusable `forge-game-input` module (joysticks, buttons, gestures, controller bridge, layouts) is a later idea, only if a second game needs it.
8. **Game controllers.** These need a native GameController bridge, because gilrs has no iOS backend.

## Risks

- **`xtool install` on a bundle xtool didn't build.** It deep-signs `.app` and `.ipa` inputs with rcodesign and rewrites the bundle ID to `XTL-<team>.<id>`, but we haven't tried it with a non-SwiftPM executable.
- **Linking a large Rust binary with `ld64.lld` against the SDK's `.tbd` stubs.** Our SwiftPM builds already link with it, but rustc drives it differently.
- **`bevy_hanabi` compute shaders on Metal.** They should work, but if they fail we can turn the particles off with a cfg in the patch.
- **Release build time on the laptop.** The first Bevy build takes several minutes.
